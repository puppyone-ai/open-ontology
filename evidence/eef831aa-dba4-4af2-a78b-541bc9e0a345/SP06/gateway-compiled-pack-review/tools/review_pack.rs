//! Independent check of a sealed compiler RolePack.
//!
//! The digest function below is the reviewer's copy of the compiler preimage.
//! It does not call `PackCatalog`'s private canonical writer. The process
//! checks are the tamper cases the approved live test does not spawn:
//! a wrong compiler string, a zeroed file SHA-256, and a README project
//! line that no longer matches the manifest.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use oo_gateway::pack_catalog::PackCatalog;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

#[test]
fn compiler_preimage_and_process_refusals() {
    let work = std::env::temp_dir().join(format!("oo-a345-review-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).expect("work");
    let fixture_dir = PathBuf::from(env_var("OO_A341_FIXTURE_DIR"));
    let (picker_text, picker_declared) = compile(&work, &fixture_dir, "visibility-picker.json", "picker.json");
    let (auditor_text, auditor_declared) = compile(&work, &fixture_dir, "visibility-auditor.json", "auditor.json");
    let picker_digest = independent_digest(&picker_text);
    let auditor_digest = independent_digest(&auditor_text);
    assert_eq!(picker_digest, picker_declared, "picker compiler digest");
    assert_eq!(auditor_digest, auditor_declared, "auditor compiler digest");
    assert_ne!(picker_digest, auditor_digest, "roles share a digest");
    assert_bindings(&picker_text, "oo_r_picker", "picker_note", "auditor_phone");
    assert_bindings(&auditor_text, "oo_r_auditor", "auditor_phone", "picker_note");
    assert!(!picker_text.contains("unclassified_pii"), "picker names the hidden column");
    assert!(!auditor_text.contains("unclassified_pii"), "auditor names the hidden column");

    let compiler = set_compiler(&picker_text, "other-compiler");
    let file = zero_file_sha(&picker_text);
    let binding = rebind_readme_project(&picker_text);
    refuse(&work, "compiler", &compiler, "compiler");
    refuse(&work, "file", &file, "source_digest");
    refuse(&work, "binding", &binding, "binding");

    let summary = "picker_digest=match\nauditor_digest=match\npicker_role=oo_r_picker\nauditor_role=oo_r_auditor\nproject=depot\npolicy=pol_depot@pv1\nknowledge=know_depot@kv1\nhidden_absent=1\ncompiler_process=refused\nfile_process=refused\nbinding_process=refused\ncompiler_reason=compiler\nfile_reason=source_digest\nbinding_reason=binding\n".to_string();
    fs::write(env_var("OO_A345_SUMMARY"), summary).expect("summary");
    let _ = fs::remove_dir_all(&work);
}

fn assert_bindings(text: &str, role: &str, present: &str, absent: &str) {
    let artifact: Value = serde_json::from_str(text).expect("artifact");
    let manifest = file_text(&artifact, "manifest.json");
    let manifest: Value = serde_json::from_str(&manifest).expect("manifest");
    assert_eq!(manifest["project"].as_str(), Some("depot"));
    assert_eq!(manifest["role"].as_str(), Some(role));
    assert_eq!(manifest["policy"]["id"].as_str(), Some("pol_depot"));
    assert_eq!(manifest["policy"]["version"].as_str(), Some("pv1"));
    assert_eq!(manifest["knowledge"]["id"].as_str(), Some("know_depot"));
    assert_eq!(manifest["knowledge"]["version"].as_str(), Some("kv1"));
    assert_eq!(manifest["compiler"].as_str(), Some("oo-builder/role-pack/v1"));
    assert_eq!(manifest["protocol"].as_str(), Some("oo.builder.role-pack-manifest/v1"));
    assert_eq!(artifact["compiler"].as_str(), Some("oo-builder/role-pack/v1"));
    assert_eq!(artifact["protocol"].as_str(), Some("oo.builder.role-pack/v1"));
    let table = file_text(&artifact, "tables/shop.orders.md");
    assert!(table.contains(&format!("`{present}`")), "visible column");
    assert!(!table.contains(&format!("`{absent}`")), "other role column");
    assert!(!table.contains("unclassified_pii"), "hidden column in the table");
}

fn independent_digest(text: &str) -> String {
    let artifact: Value = serde_json::from_str(text).expect("artifact");
    let files = artifact["files"].as_array().expect("files");
    let mut records = Vec::new();
    for file in files {
        let body = file["text"].as_str().expect("text");
        let sha = sha256_hex(body.as_bytes());
        assert_eq!(sha, file["sha256"].as_str().expect("declared file sha"));
        assert_eq!(file["bytes"].as_u64().expect("bytes"), body.len() as u64);
        records.push(serde_json::json!({
            "bytes": body.len() as u64,
            "path": file["path"].as_str().expect("path"),
            "sha256": sha,
        }));
    }
    records.sort_by(|left, right| left["path"].as_str().expect("path").cmp(right["path"].as_str().expect("path")));
    let digest = sha256_hex(canonical_json(&Value::Array(records)).as_bytes());
    assert_eq!(Some(digest.as_str()), artifact["pack_digest"].as_str());
    let manifest = file_text(&artifact, "manifest.json");
    assert_eq!(
        artifact["manifest_sha256"].as_str(),
        Some(sha256_hex(manifest.as_bytes()).as_str())
    );
    digest
}

fn file_text(artifact: &Value, path: &str) -> String {
    artifact["files"]
        .as_array()
        .expect("files")
        .iter()
        .find(|file| file["path"] == path)
        .unwrap_or_else(|| panic!("missing {path}"))["text"]
        .as_str()
        .expect("text")
        .to_owned()
}

fn set_compiler(text: &str, compiler: &str) -> String {
    let mut artifact: Value = serde_json::from_str(text).expect("artifact");
    artifact["compiler"] = Value::String(compiler.to_owned());
    serde_json::to_string(&artifact).expect("compiler rewrite")
}

fn zero_file_sha(text: &str) -> String {
    let mut artifact: Value = serde_json::from_str(text).expect("artifact");
    artifact["files"][0]["sha256"] = Value::String("0".repeat(64));
    serde_json::to_string(&artifact).expect("file rewrite")
}

fn rebind_readme_project(text: &str) -> String {
    let mut artifact: Value = serde_json::from_str(text).expect("artifact");
    let mut old_sha = String::new();
    let mut new_sha = String::new();
    {
        let files = artifact["files"].as_array_mut().expect("files");
        for file in files.iter_mut() {
            if file["path"].as_str() != Some("README.md") {
                continue;
            }
            let body = file["text"].as_str().expect("readme");
            assert_eq!(body.matches("project: `depot`").count(), 1, "one project line");
            old_sha = file["sha256"].as_str().expect("old sha").to_owned();
            let body = body.replacen("project: `depot`", "project: `other`", 1);
            new_sha = sha256_hex(body.as_bytes());
            assert_ne!(old_sha, new_sha, "project rebinding changed no bytes");
            assert_eq!(file["bytes"].as_u64(), Some(body.len() as u64), "readme length");
            file["text"] = Value::String(body);
            file["sha256"] = Value::String(new_sha.clone());
        }
    }
    assert_eq!(old_sha.len(), 64, "readme");
    let mut manifest_sha = String::new();
    {
        let files = artifact["files"].as_array_mut().expect("files");
        for file in files.iter_mut() {
            if file["path"].as_str() != Some("manifest.json") {
                continue;
            }
            let manifest = file["text"].as_str().expect("manifest");
            assert_eq!(manifest.matches(old_sha.as_str()).count(), 1, "manifest lists the readme sha once");
            assert!(manifest.contains("\"project\":\"depot\""), "manifest project stays depot");
            let manifest = manifest.replacen(&old_sha, &new_sha, 1);
            manifest_sha = sha256_hex(manifest.as_bytes());
            file["text"] = Value::String(manifest);
            file["sha256"] = Value::String(manifest_sha.clone());
        }
    }
    assert_eq!(manifest_sha.len(), 64, "manifest");
    artifact["manifest_sha256"] = Value::String(manifest_sha);
    let files = artifact["files"].as_array().expect("files");
    let mut records = Vec::new();
    let mut total = 0_u64;
    for file in files {
        let bytes = file["bytes"].as_u64().expect("bytes");
        total += bytes;
        records.push(serde_json::json!({
            "bytes": bytes,
            "path": file["path"].as_str().expect("path"),
            "sha256": file["sha256"].as_str().expect("sha"),
        }));
    }
    records.sort_by(|left, right| left["path"].as_str().expect("path").cmp(right["path"].as_str().expect("path")));
    artifact["pack_digest"] = Value::String(sha256_hex(canonical_json(&Value::Array(records)).as_bytes()));
    artifact["total_bytes"] = Value::from(total);
    serde_json::to_string(&artifact).expect("binding rewrite")
}

fn refuse(work: &Path, label: &str, pack: &str, reason: &str) {
    let dir = work.join(label);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("packs")).expect("config dir");
    fs::copy(env_var("OO_A341_RUNTIME_PASSWORD_FILE"), dir.join("runtime.pass")).expect("runtime");
    fs::copy(env_var("OO_A341_CA_FILE"), dir.join("ca.crt")).expect("ca");
    fs::write(dir.join("endpoint.json"), "{\"host\":\"127.0.0.1\",\"port\":1,\"database\":\"oo_a345\"}\n").expect("endpoint");
    fs::write(dir.join("packs/picker.json"), pack).expect("pack");
    let loaded = PackCatalog::load_dir(&dir.join("packs")).expect_err("tampered pack loaded");
    assert_eq!(loaded.reason, reason, "{label}");
    let config = "{\"listen\":\"127.0.0.1:0\",\"audience\":\"https://gateway.loopback.invalid/v1\",\"runtime_user\":\"oo_token_runtime\",\"password_file\":\"runtime.pass\",\"ca_file\":\"ca.crt\",\"endpoint_file\":\"endpoint.json\",\"pack_dir\":\"packs\",\"body_limit\":1048576,\"request_timeout_ms\":5000,\"global_concurrency\":1}\n";
    fs::write(dir.join("config.json"), config).expect("config");
    let mut child = ChildGuard(spawn_gateway(&dir));
    let started = Instant::now();
    let code = loop {
        if let Some(status) = child.0.try_wait().expect("wait") {
            break status.code();
        }
        assert!(!dir.join("listen.addr").exists(), "{label} wrote listen.addr");
        assert!(started.elapsed() <= Duration::from_secs(5), "{label} hung");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(code, Some(1), "{label} exit");
    assert!(!dir.join("listen.addr").exists(), "{label} listen.addr after exit");
    let stderr = fs::read_to_string(dir.join("stderr.txt")).unwrap_or_default();
    assert!(stderr.contains("oo-gateway: packs"), "{label} stderr: {stderr}");
    let runtime = fs::read_to_string(env_var("OO_A341_RUNTIME_PASSWORD_FILE")).expect("runtime read");
    let trimmed = runtime.trim_end_matches(['\n', '\r']);
    assert!(!stderr.contains(trimmed), "{label} password in stderr");
}

fn compile(work: &Path, fixture_dir: &Path, visibility: &str, output_name: &str) -> (String, String) {
    let output = work.join(output_name);
    let digest_path = work.join(format!("{output_name}.digest"));
    let produced = Command::new(env_var("OO_A341_FIXTURE_BIN"))
        .arg("compile")
        .arg("--compiler")
        .arg(env_var("OO_A341_ROLE_PACK_BIN"))
        .arg("--effective")
        .arg(fixture_dir.join("effective.json"))
        .arg("--visibility")
        .arg(fixture_dir.join(visibility))
        .arg("--output")
        .arg(&output)
        .arg("--digest")
        .arg(&digest_path)
        .stdin(Stdio::null())
        .output()
        .expect("fixture");
    let tool_stderr = String::from_utf8_lossy(&produced.stderr);
    assert!(produced.status.success(), "compile {visibility}: {tool_stderr}");
    let text = fs::read_to_string(&output).expect("pack");
    let digest = fs::read_to_string(&digest_path).expect("digest");
    let digest = digest.trim().to_owned();
    assert_eq!(digest.len(), 64);
    (text, digest)
}

fn spawn_gateway(dir: &Path) -> Child {
    let bin = env!("CARGO_BIN_EXE_oo-gateway");
    let stderr = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("stderr.txt"))
        .expect("stderr");
    let path = std::env::var("PATH").unwrap_or_default();
    Command::new(bin)
        .arg(dir)
        .env_clear()
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn")
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_string(out, key);
        out.push(':');
        write_value(out, &map[*key]);
    }
    out.push('}');
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
}
