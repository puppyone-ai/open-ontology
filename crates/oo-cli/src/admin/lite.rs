//! Single-owner compose foundation. Never calls it complete stack acceptance.
use super::{Failure, Invocation};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) fn run(
    action: &str,
    path: &Path,
    stack: &oo_stack::Stack,
) -> Result<Invocation, Failure> {
    if stack.deploy.target != "local" || stack.deploy.profile != "lite" {
        return Err(Failure::local("LITE_LOCAL_ONLY"));
    }
    let config = fs::canonicalize(path).map_err(|_| Failure::local("ADMIN_CONFIG_IO"))?;
    let state = config
        .parent()
        .ok_or_else(|| Failure::local("ADMIN_CONFIG_IO"))?
        .join(".oo");
    let root = state.join("lite");
    let image = std::env::var("OO_LITE_RUNTIME_IMAGE").unwrap_or_else(|_| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| fs::read_to_string(exe.with_file_name("oo-lite-image")).ok())
            .unwrap_or_default()
            .trim()
            .to_owned()
    });
    if action == "up" && !digest(&image) {
        return Err(Failure::local(
            "LITE_IMAGE_DIGEST_REQUIRED: install the repository-built image and set OO_LITE_RUNTIME_IMAGE",
        ));
    }
    if action == "up" {
        require_images(&image)?;
        let store = oo_stack::secret::SecretStore::from_home().map_err(Failure::local)?;
        store
            .read(&stack.model.key.secret_ref)
            .map_err(Failure::local)?;
        if let oo_stack::stack::ModelEndpoint::Secret(reference) = &stack.model.base_url {
            store.read(&reference.secret_ref).map_err(Failure::local)?;
        }
        for source in &stack.sources {
            store
                .read(&source.password.secret_ref)
                .map_err(Failure::local)?;
            if let Some(ca) = &source.tls_ca {
                store.read(&ca.secret_ref).map_err(Failure::local)?;
            }
        }
        for dir in [&state, &root] {
            match fs::DirBuilder::new().mode(0o700).create(dir) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(_) => return Err(Failure::local("LITE_STATE_DIRECTORY")),
            }
            let meta =
                fs::symlink_metadata(dir).map_err(|_| Failure::local("LITE_STATE_DIRECTORY"))?;
            if !meta.is_dir() || meta.permissions().mode() & 0o777 != 0o700 {
                return Err(Failure::local("LITE_STATE_DIRECTORY"));
            }
        }
        oo_stack::credentials::bootstrap(&root.join("pki")).map_err(Failure::local)?;
    }
    let request = json!({"action":action,"root":root,"stack":stack,"image":image,
    "templates":{
        "compose.yaml":include_str!("../../../../deploy/lite/compose.yaml"),
        "postgres.sh":include_str!("../../../../deploy/lite/postgres.sh"),
        "scheduler.py":include_str!("../../../../deploy/lite/scheduler.py"),
    }});
    let mut child = Command::new("python3")
        .args(["-c", include_str!("../../../../deploy/lite/lifecycle.py")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Failure::local("LITE_START"))?;
    let result = child
        .stdin
        .take()
        .ok_or_else(|| Failure::local("LITE_STDIN"))?
        .write_all(request.to_string().as_bytes());
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Failure::local("LITE_STDIN"));
    }
    let out = child
        .wait_with_output()
        .map_err(|_| Failure::local("LITE_WAIT"))?;
    let mut report: Value =
        serde_json::from_slice(&out.stdout).map_err(|_| Failure::local("LITE_PROTOCOL"))?;
    if root.join("pki").exists() {
        report["bootstrap_credentials"] = match oo_stack::credentials::status(&root.join("pki")) {
            Ok(status) => json!(status),
            Err(_) => json!({"status":"invalid","nearest_credential_expiry":null}),
        };
    }
    Ok(Invocation::AdminReport(
        report.to_string(),
        if out.status.success() { 0 } else { 1 },
    ))
}

fn digest(image: &str) -> bool {
    let value = if let Some((repository, value)) = image.split_once('@') {
        if repository.is_empty()
            || !repository
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"./:_-".contains(&b))
            || !repository.as_bytes()[0].is_ascii_alphanumeric()
        {
            return false;
        }
        value
    } else {
        image
    };
    value.strip_prefix("sha256:").is_some_and(|v| {
        v.len() == 64
            && v.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn image_help(image: &str) -> String {
    if image.contains('@') {
        format!("docker pull {image}")
    } else {
        format!(
            "load the matching release image archive with docker load --input PATH (required ID {image}); \
             a local image ID cannot be pulled from a registry"
        )
    }
}

fn require_images(runtime: &str) -> Result<(), Failure> {
    let postgres = include_str!("../../../../deploy/lite/compose.yaml")
        .lines()
        .filter_map(|line| line.trim().strip_prefix("image: "))
        .find(|image| image.contains("postgres@sha256:"))
        .filter(|image| digest(image))
        .ok_or_else(|| Failure::local("LITE_COMPOSE_TEMPLATE"))?;
    let mut info = Command::new("docker");
    info.args(["info", "--format", "{{.OSType}}"]);
    let (exit, _, _) = super::build_retained::invoke(info, vec![], 30)
        .map_err(|_| Failure::local("LITE_DOCKER_UNAVAILABLE: start Docker and retry oo up"))?;
    if exit != 0 {
        return Err(Failure::local(
            "LITE_DOCKER_UNAVAILABLE: start Docker and retry oo up",
        ));
    }
    let mut missing = Vec::new();
    for image in [runtime, postgres] {
        let mut inspect = Command::new("docker");
        inspect.args(["image", "inspect", image]);
        let (exit, _, _) = super::build_retained::invoke(inspect, vec![], 30)?;
        if exit != 0 {
            missing.push(image_help(image));
        }
    }
    if !missing.is_empty() {
        return Err(Failure::local(format!(
            "LITE_IMAGES_MISSING: {}; then retry oo up. No services were started.",
            missing.join("; ")
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_must_be_content_addressed() {
        assert!(digest(&format!("sha256:{}", "a".repeat(64))));
        assert!(digest(&format!(
            "example/runtime@sha256:{}",
            "a".repeat(64)
        )));
        for image in ["latest", "repo:tag", "sha256:abc", ""] {
            assert!(!digest(image));
        }
        assert!(!digest(&format!("bad;command@sha256:{}", "a".repeat(64))));
        assert!(!digest(&format!("-option@sha256:{}", "a".repeat(64))));
    }

    #[test]
    fn missing_image_guidance_distinguishes_registry_refs_from_local_ids() {
        let id = format!("sha256:{}", "a".repeat(64));
        assert!(image_help(&id).contains("docker load --input PATH"));
        assert!(!image_help(&id).contains("docker pull"));
        let reference = format!("registry.example/runtime@{id}");
        assert_eq!(image_help(&reference), format!("docker pull {reference}"));
    }
}
