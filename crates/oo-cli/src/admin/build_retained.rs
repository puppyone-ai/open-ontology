//! Subprocess boundary for the retained Loader, Builder and publisher.
use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::DirBuilderExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use serde_json::{Value, json};

use super::{Failure, review};
type Result<T> = std::result::Result<T, Failure>;

pub(super) fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty() && !s.starts_with('-'))
        .ok_or_else(|| Failure::local("BUILD_RUNTIME_CONFIG"))
}

pub(super) fn binary(name: &str) -> Result<PathBuf> {
    Ok(std::env::current_exe()
        .map_err(|_| Failure::local("BUILD_INSTALL"))?
        .with_file_name(name))
}

/// Drain both pipes while waiting, bound output and kill only our process group.
pub(super) fn invoke(
    mut command: Command,
    input: Vec<u8>,
    seconds: u64,
) -> Result<(i32, Vec<u8>, Vec<u8>)> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| Failure::local("BUILD_COMMAND_START"))?;
    let pid = child.id();
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Failure::local("BUILD_COMMAND_PIPE"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Failure::local("BUILD_COMMAND_PIPE"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Failure::local("BUILD_COMMAND_PIPE"))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let read = |stream: Box<dyn Read + Send>| {
            let mut bytes = Vec::new();
            stream
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        };
        let errors = std::thread::spawn(move || read(Box::new(stderr)));
        let out = read(Box::new(stdout));
        let err = errors.join();
        let status = child.wait();
        let written = writer.join();
        let _ = tx.send((status, out, err, written));
    });
    let result = rx.recv_timeout(Duration::from_secs(seconds));
    let Ok((Ok(status), Ok(out), Ok(Ok(err)), Ok(Ok(())))) = result else {
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .status();
        return Err(Failure::local("BUILD_COMMAND_TIMEOUT_OR_IO"));
    };
    if out.len() > 64 * 1024 * 1024 || err.len() > 64 * 1024 * 1024 {
        return Err(Failure::local("BUILD_COMMAND_OUTPUT_LIMIT"));
    }
    Ok((status.code().unwrap_or(1), out, err))
}

pub(super) fn save(path: &Path, value: &Value) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let parent = path
        .parent()
        .ok_or_else(|| Failure::local("BUILD_ARTIFACT_PATH"))?;
    for ancestor in parent.ancestors() {
        if fs::symlink_metadata(ancestor).is_ok_and(|m| !m.is_dir()) {
            return Err(Failure::local("BUILD_STATE_DIRECTORY"));
        }
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|_| Failure::local("BUILD_STATE_DIRECTORY"))?;
    if fs::symlink_metadata(parent)
        .map_err(|_| Failure::local("BUILD_STATE_DIRECTORY"))?
        .mode()
        & 0o777
        != 0o700
    {
        return Err(Failure::local("BUILD_STATE_DIRECTORY"));
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    review::save_new(&temporary, value)?;
    fs::rename(temporary, path)
        .and_then(|_| fs::File::open(parent)?.sync_all())
        .map_err(|_| Failure::local("BUILD_STATE_IO"))
}

pub(super) fn load(
    root: &Path,
    runtime: &Value,
    stack: &oo_stack::Stack,
    target: &Path,
    generation: &str,
    candidate: bool,
    cycle: bool,
) -> Result<Value> {
    let stack_path = root.join("stack.json");
    save(&stack_path, &json!(stack))?;
    let mut command = Command::new(binary("oo-load")?);
    if candidate {
        command.arg("publish").args([
            "--generation",
            generation,
            "--library",
            text(runtime, "library_id")?,
        ]);
    } else if cycle {
        let policy = root.join("c5.json");
        save(&policy, &json!({"cycle":1,"full_read_every":1}))?;
        command.arg("sync").arg("--c5").arg(policy);
    }
    command
        .arg("--stack")
        .arg(stack_path)
        .args(["--source", text(runtime, "source_id")?])
        .arg("--target")
        .arg(target)
        .args(["--secrets-dir", text(runtime, "secrets_dir")?]);
    if !candidate {
        command.args(["--run-id", generation]);
    }
    let (code, out, err) = invoke(command, vec![], 1800)?;
    if candidate {
        let value: Value =
            serde_json::from_slice(&out).map_err(|_| Failure::local("BUILD_CANDIDATE_RECEIPT"))?;
        let refusal: Value = serde_json::from_slice(&err).unwrap_or(Value::Null);
        if (code != 0 && refusal["error"] != "publication_semantic_admission_required")
            || !matches!(value["state"].as_str(), Some("pending" | "ready"))
        {
            return Err(Failure::local("BUILD_CANDIDATE_REFUSED"));
        }
        Ok(value)
    } else if code == 0 {
        Ok(json!({"generation":generation,"loaded":true}))
    } else {
        Err(Failure::local("BUILD_LOAD_FAILED"))
    }
}

pub(super) fn builder(
    root: &Path,
    runtime: &Value,
    stack: &oo_stack::Stack,
    generation: &str,
    origin: Option<&Path>,
) -> Result<Value> {
    let mut config = runtime["builder"].clone();
    config["protocol"] = json!("oo.builder.retained-build/v1");
    config["library_id"] = runtime["library_id"].clone();
    config["generation"] = json!(generation);
    let source = stack
        .sources
        .iter()
        .find(|s| Some(s.id.as_str()) == runtime["source_id"].as_str())
        .ok_or_else(|| Failure::local("BUILD_RUNTIME_SOURCE"))?;
    config["source_engine"] = json!(source.engine);
    if let Some(origin) = origin {
        let mut pins = json!({});
        for (name, file) in [
            ("facts", "builder/facts.json"),
            ("reviewed_registry", "reviewed-registry.json"),
        ] {
            let path = origin.join(file);
            let bytes = fs::read(&path).map_err(|_| Failure::local("BUILD_ORIGIN_MISSING"))?;
            pins[name] = json!({"path":path,"sha256":oo_core::api::payload_sha256(&bytes)});
        }
        config["origin"] = pins;
    }
    let output = root.join("builder");
    // The shared Builder cannot restart an incomplete output directory safely.
    // Recover completed output via the durable wrapper receipt, never spend again.
    let receipt = root.join("builder-result.json");
    if receipt.exists() {
        return review::read(&receipt);
    }
    if output.exists() {
        return Err(Failure::local(
            "BUILD_BUILDER_INTERRUPTED_REQUIRES_RECONCILIATION",
        ));
    }
    let config_path = root.join("builder-config.json");
    save(&config_path, &config)?;
    let mut command = Command::new("python3");
    // Save the result in the child before returning, including on parent SIGKILL.
    command.args(["-c", "import importlib.util,json,os,sys\np,c,o,r=sys.argv[1:]\nsys.path.insert(0,os.path.dirname(p))\ns=importlib.util.spec_from_file_location('retained_build',p); m=importlib.util.module_from_spec(s); s.loader.exec_module(m)\nos.umask(0o077)\nv=m.run(c,o)\nm.save(r,v)\nprint(json.dumps(v))"])
        .arg(text(runtime, "builder_script")?).arg(config_path).arg(output).arg(&receipt)
        .env("OO_SELFBUILD_BUDGET_BIN", binary("selfbuild-budget")?);
    let (code, _, _) = invoke(command, vec![], 1800)?;
    if code != 0 {
        return Err(Failure::local("BUILD_BUILDER_FAILED"));
    }
    review::read(&receipt)
}

pub(super) fn publish(
    root: &Path,
    runtime: &Value,
    generation: &str,
    reviewed: &Value,
) -> Result<Value> {
    let mut command = Command::new(binary("release-coordinator")?);
    command.args([
        "publish-retained",
        "--config",
        text(runtime, "publisher_config")?,
    ]);
    let request = json!({"protocol":"oo.release-coordinator/publish-retained/v1",
        "library_id":runtime["library_id"],"generation":generation,
        "login":runtime["gateway_login"],"reviewed_registry":reviewed});
    let (code, bytes, _) = invoke(command, request.to_string().into_bytes(), 120)?;
    if code != 0 {
        return Err(Failure::local(if code == 2 {
            "BUILD_PUBLICATION_REFUSED_UNCHANGED"
        } else {
            "BUILD_PUBLICATION_FAILED"
        }));
    }
    let receipt: Value =
        serde_json::from_slice(&bytes).map_err(|_| Failure::local("BUILD_PUBLICATION_RECEIPT"))?;
    let document = &receipt["gateway_document"];
    if !matches!(receipt["outcome"].as_str(), Some("activated" | "replay"))
        || receipt["state"] != "ready"
        || receipt["library_id"] != runtime["library_id"]
        || receipt["generation"] != generation
        || document["protocol"] != "oo.gateway.library-reviewed/v1"
        || document["envelope"]["producer"]["artifact"].is_null()
    {
        return Err(Failure::local("BUILD_PUBLICATION_RECEIPT"));
    }
    save(&root.join("publisher-receipt.json"), &receipt)?;
    let gateway_root = Path::new(text(&runtime["gateway"], "runtime_dir")?);
    save(
        &gateway_root.join("packs/library.json"),
        &document["envelope"]["producer"]["artifact"],
    )?;
    save(
        &gateway_root.join("reviewed/library/library.json"),
        document,
    )?;
    let mut publication = runtime["gateway"].clone();
    publication["protocol"] = json!("oo.build-publication/v1");
    publication["published"] = json!(true);
    publication["receipt_sha256"] = receipt["receipt_sha256"].clone();
    Ok(publication)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn subprocess_exit_codes_and_private_atomic_replacement() {
        let mut command = Command::new("python3");
        command.args(["-c", "import sys; assert sys.stdin.read()=='request'; print('receipt'); sys.stderr.write('refused'); sys.exit(2)"]);
        let result = invoke(command, b"request".to_vec(), 10).ok().unwrap();
        assert_eq!(result, (2, b"receipt\n".to_vec(), b"refused".to_vec()));
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../evidence")
            .join(format!("retained-save-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let path = root.join("receipt.json");
        save(&path, &json!({"state":"pending"})).ok().unwrap();
        save(&path, &json!({"state":"ready"})).ok().unwrap();
        assert_eq!(review::read(&path).ok().unwrap()["state"], "ready");
        symlink(&root, root.join("link")).unwrap();
        assert!(save(&root.join("link/unsafe.json"), &json!({})).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn timeout_is_not_a_success() {
        let mut command = Command::new("python3");
        command.args(["-c", "import time; time.sleep(60)"]);
        assert!(invoke(command, vec![], 1).is_err());
    }
}
