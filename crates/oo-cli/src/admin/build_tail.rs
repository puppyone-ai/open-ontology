//! Private lifecycle receipts. Enrollment still refuses ambiguous retries.
use std::fs;
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use super::{Failure, review};

type Result<T> = std::result::Result<T, Failure>;

fn once(
    root: &Path,
    name: &str,
    binding: &Value,
    execute: impl FnOnce() -> Result<Value>,
) -> Result<Value> {
    let output = root.join(format!("{name}-receipt.json"));
    let fingerprint = review::hash(binding);
    if output.exists() {
        let receipt = review::read(&output)?;
        if receipt["input_sha256"] != fingerprint || receipt["result"].is_null() {
            return Err(Failure::local("BUILD_TAIL_RECEIPT_BINDING"));
        }
        return Ok(receipt["result"].clone());
    }
    let intent = root.join(format!("{name}-intent.json"));
    if intent.exists() {
        // Enrollment has no replay material. Unlike first-user issuance, do not
        // retry it after a lost response or a crash before the private receipt.
        return Err(Failure::local("BUILD_ISSUANCE_UNCERTAIN_RECONCILE_AUDIT"));
    }
    review::save_new(&intent, &json!({"input_sha256":fingerprint}))?;
    fs::File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| Failure::local("BUILD_STATE_IO"))?;
    let result = execute()?;
    review::save_new(
        &output,
        &json!({"input_sha256":fingerprint,"result":result}),
    )?;
    fs::File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| Failure::local("BUILD_STATE_IO"))?;
    Ok(result)
}

pub(super) fn credential_hash(path: &Path) -> Result<String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Failure::local("BUILD_TOKEN_FILE"))?;
    let meta = file
        .metadata()
        .map_err(|_| Failure::local("BUILD_TOKEN_FILE"))?;
    if !meta.is_file() || meta.nlink() != 1 || meta.mode() & 0o777 != 0o600 || meta.len() != 48 {
        return Err(Failure::local("BUILD_TOKEN_FILE"));
    }
    let mut bytes = Vec::new();
    file.take(49)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::local("BUILD_TOKEN_FILE"))?;
    if !bytes.starts_with(b"ooct_")
        || !bytes[5..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(b))
    {
        return Err(Failure::local("BUILD_TOKEN_FILE"));
    }
    Ok(oo_core::api::payload_sha256(&bytes))
}

pub(super) fn verify_token(root: &Path) -> Result<()> {
    let receipt = review::read(&root.join("first_token-receipt.json"))?;
    if receipt["result"]["token_sha256"] != credential_hash(&root.join("first-user.token"))? {
        return Err(Failure::local("BUILD_TOKEN_RECEIPT"));
    }
    Ok(())
}

fn lifecycle(root: &Path, publication: &Value, command: &[&str]) -> Result<Vec<u8>> {
    let client = root.join("client");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&client)
        .map_err(|_| Failure::local("BUILD_CLIENT_DIRECTORY"))?;
    let meta =
        fs::symlink_metadata(&client).map_err(|_| Failure::local("BUILD_CLIENT_DIRECTORY"))?;
    if !meta.is_dir() || meta.mode() & 0o777 != 0o700 {
        return Err(Failure::local("BUILD_CLIENT_DIRECTORY"));
    }
    let mut process =
        Command::new(std::env::current_exe().map_err(|_| Failure::local("BUILD_INSTALL"))?);
    process.args(command);
    for (flag, field) in [
        ("--gateway", "gateway_url"),
        ("--ca-file", "ca_file"),
        ("--client-pem", "client_pem"),
    ] {
        let value = publication[field]
            .as_str()
            .filter(|s| !s.is_empty() && !s.starts_with('-'))
            .ok_or_else(|| Failure::local("BUILD_PUBLICATION_RECEIPT"))?;
        process.args([flag, value]);
    }
    process
        .arg("--grant-file")
        .arg(root.join("first-user-grant.json"));
    let output = process
        .env("OO_CONFIG_DIR", client)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| Failure::local("BUILD_LIFECYCLE_START"))?;
    if !output.status.success() || output.stdout.len() > 1_048_576 {
        return Err(Failure::local("BUILD_LIFECYCLE_FAILED_RECONCILE_AUDIT"));
    }
    Ok(output.stdout)
}

pub(super) fn gateway(publication: &Value) -> Result<Value> {
    use super::build_retained::{binary, text};
    use std::time::{Duration, Instant};
    if publication["protocol"] != "oo.build-publication/v1" || publication["published"] != true {
        return Err(Failure::local("BUILD_PUBLICATION_RECEIPT"));
    }
    let root = Path::new(text(publication, "runtime_dir")?);
    let document = review::read(&root.join("reviewed/library/library.json"))?;
    if document["publication"]["receipt_sha256"] != publication["receipt_sha256"] {
        return Err(Failure::local("BUILD_GATEWAY_PUBLICATION_BINDING"));
    }
    let url = reqwest::Url::parse(text(publication, "gateway_url")?)
        .map_err(|_| Failure::local("BUILD_GATEWAY_URL"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(Failure::local("BUILD_GATEWAY_URL"));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Failure::local("BUILD_GATEWAY_RUNTIME"))?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .tls_certs_only(
            reqwest::Certificate::from_pem_bundle(&crate::load::read_ca_file(text(
                publication,
                "ca_file",
            )?)?)
            .map_err(|_| Failure::local("BUILD_GATEWAY_TLS"))?,
        )
        .identity(
            reqwest::Identity::from_pem(&crate::load::read_client_pem(text(
                publication,
                "client_pem",
            )?)?)
            .map_err(|_| Failure::local("BUILD_GATEWAY_TLS"))?,
        )
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| Failure::local("BUILD_GATEWAY_TLS"))?;
    let health = url
        .join("/ready")
        .map_err(|_| Failure::local("BUILD_GATEWAY_URL"))?;
    let ready = || {
        runtime.block_on(async {
            client
                .get(health.clone())
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
        })
    };
    // Reviewed files and Packs reload per request. An already healthy owned
    // endpoint needs no restart and retains in-flight request snapshots.
    if !ready() {
        let mut child = Command::new(binary("oo-gateway")?)
            .arg(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Failure::local("BUILD_GATEWAY_START"))?;
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            if ready() {
                break;
            }
            if child
                .try_wait()
                .map_err(|_| Failure::local("BUILD_GATEWAY_START"))?
                .is_some()
            {
                return Err(Failure::local("BUILD_GATEWAY_START"));
            }
            if Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Failure::local("BUILD_GATEWAY_NOT_READY"));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let mut receipt = publication.clone();
    receipt["gateway_verified"] = json!(true);
    receipt["verification"] = json!("mtls_ready; data query not yet measured");
    Ok(receipt)
}

pub(super) fn run(root: &Path, name: &str, publication: &Value) -> Result<Value> {
    if publication["protocol"] != "oo.build-publication/v1"
        || publication["published"] != true
        || publication["gateway_verified"] != true
        || !publication["first_user_grant"].is_object()
    {
        return Err(Failure::local("BUILD_PUBLICATION_RECEIPT"));
    }
    let grant = root.join("first-user-grant.json");
    if grant.exists() {
        if review::read(&grant)? != publication["first_user_grant"] {
            return Err(Failure::local("BUILD_TAIL_RECEIPT_BINDING"));
        }
    } else {
        review::save_new(&grant, &publication["first_user_grant"])?;
    }
    match name {
        "first_token" => super::build_token::run(root, publication),
        "share_prompt" => once(root, name, publication, || {
            // Lane B issues a separate short-lived enrollment code, not the
            // first user's long-lived token. Keep the entire prompt private.
            let raw = lifecycle(root, publication, &["share-prompt"])?;
            let mut prompt =
                String::from_utf8(raw).map_err(|_| Failure::local("BUILD_PROMPT_RECEIPT"))?;
            if !prompt.contains("--code-stdin") || prompt.contains("ooct_") {
                return Err(Failure::local("BUILD_PROMPT_RECEIPT"));
            }
            // share-prompt omits private-CA/mTLS flags; the printed login must
            // carry those references, not their credential contents.
            let shell = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
            // A runtime-executed build names the consumer's own files as the
            // owner sees them; the operator identity stays inside the runtime.
            let named = |prompt: &str, field: &str| {
                if publication.get(prompt).is_some() {
                    super::build_retained::text(publication, prompt)
                } else {
                    super::build_retained::text(publication, field)
                }
            };
            let flags = format!(
                "--code-stdin --ca-file {} --client-pem {}",
                shell(named("prompt_ca_file", "ca_file")?),
                shell(named("prompt_client_pem", "client_pem")?)
            );
            prompt = prompt.replace("--code-stdin", &flags);
            Ok(json!({"prompt":prompt}))
        }),
        _ => Err(Failure::local("BUILD_TAIL_STAGE")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn receipt_child() {
        let Some(root) = std::env::var_os("OO_TAIL_TEST_ROOT") else {
            return;
        };
        let name = std::env::var("OO_TAIL_TEST_STAGE").unwrap();
        let root = Path::new(&root);
        let _ = once(root, &name, &json!({"fixture":true}), || {
            fs::write(root.join("dispatched"), b"1").unwrap();
            if std::env::var_os("OO_TAIL_TEST_INTERRUPT").is_some() {
                loop {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            Ok(json!({"done":name}))
        });
        loop {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn sigkill_at_each_tail_receipt_boundary_never_duplicates_dispatch() {
        let base = // A private temp dir, not the repository evidence tree.
        std::fs::canonicalize(std::env::temp_dir()).unwrap()
            .join(format!("oo-tail-kill-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
        for name in ["publish", "gateway", "first_token", "share_prompt"] {
            for interrupt in [false, true] {
                let root = base.join(format!("{name}-{interrupt}"));
                fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .args(["--exact", "args::admin::build_tail::tests::receipt_child"])
                    .env("OO_TAIL_TEST_ROOT", &root)
                    .env("OO_TAIL_TEST_STAGE", name)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                if interrupt {
                    command.env("OO_TAIL_TEST_INTERRUPT", "1");
                }
                let mut child = command.spawn().unwrap();
                let receipt = root.join(format!("{name}-receipt.json"));
                let signal = if interrupt {
                    root.join("dispatched")
                } else {
                    receipt
                };
                let deadline = Instant::now() + Duration::from_secs(10);
                let ready = || {
                    if interrupt {
                        fs::read(&signal).is_ok_and(|bytes| bytes == b"1")
                    } else {
                        review::read(&signal).is_ok()
                    }
                };
                while !ready() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                child.kill().unwrap();
                child.wait().unwrap();
                assert!(ready(), "child did not reach boundary");
                let resumed = once(&root, name, &json!({"fixture":true}), || {
                    panic!("duplicate")
                });
                assert_eq!(resumed.is_err(), interrupt);
                assert_eq!(fs::read(root.join("dispatched")).unwrap(), b"1");
            }
        }
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn resumes_each_tail_boundary_without_reissuing_and_refuses_ambiguous_calls() {
        let root = // A private temp dir, not the repository evidence tree.
        std::fs::canonicalize(std::env::temp_dir()).unwrap()
            .join(format!("oo-tail-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let binding = json!({"fixture":"not_a_publication"});
        for name in ["publish", "gateway", "first_token", "share_prompt"] {
            let result = once(&root, name, &binding, || Ok(json!({"done":name})))
                .ok()
                .unwrap();
            let recovered = once(&root, name, &binding, || panic!("must not reissue"))
                .ok()
                .unwrap();
            assert_eq!(result, recovered);
            assert!(once(&root, name, &json!({"changed":true}), || panic!()).is_err());
        }
        assert!(
            once(&root, "lost-response", &binding, || Err(Failure::local(
                "TEST"
            )))
            .is_err()
        );
        assert!(
            once(&root, "lost-response", &binding, || panic!(
                "ambiguous retry"
            ))
            .is_err()
        );
        assert!(gateway(&binding).is_err());
        assert!(run(&root, "first_token", &binding).is_err());
        use std::io::Write;
        let path = root.join("first-user.token");
        let mut token = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        token
            .write_all(format!("ooct_{}", "a".repeat(43)).as_bytes())
            .unwrap();
        let hash = credential_hash(&path).ok().unwrap();
        // Replace only the synthetic receipt created by this test.
        fs::remove_file(root.join("first_token-receipt.json")).unwrap();
        review::save_new(
            &root.join("first_token-receipt.json"),
            &json!({"result":{"token_sha256":hash}}),
        )
        .ok()
        .unwrap();
        assert!(verify_token(&root).is_ok());
        fs::write(&path, format!("ooct_{}", "b".repeat(43))).unwrap();
        assert!(verify_token(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
