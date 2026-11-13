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
    run_mode(action, path, stack, false)
}

pub(super) fn run_demo(
    action: &str,
    path: &Path,
    stack: &oo_stack::Stack,
) -> Result<Invocation, Failure> {
    run_mode(action, path, stack, true)
}

fn run_mode(
    action: &str,
    path: &Path,
    stack: &oo_stack::Stack,
    demo: bool,
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
    let image = pinned_image();
    if action == "up" && !digest(&image) {
        return Err(Failure::local(
            "LITE_IMAGE_DIGEST_REQUIRED: install the repository-built image and set OO_LITE_RUNTIME_IMAGE",
        ));
    }
    if action == "up" {
        require_images(&image)?;
        if !demo {
            let store = super::wizard::owner_secret_store()?;
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
    }
    // One helper per process: credential creation and the status report below.
    let saved = saved_image(&root);
    let _openssl = if action == "up" {
        Some(OpensslHelper::start_lane(
            &root.join("pki"),
            &image,
            demo.then_some("v03"),
        )?)
    } else if root.join("pki").exists() {
        OpensslHelper::start(&root.join("pki"), saved.as_deref().unwrap_or(&image)).ok()
    } else {
        None
    };
    if action == "up" {
        oo_stack::credentials::bootstrap(&root.join("pki")).map_err(Failure::local)?;
        // The owner's CLI gets its own workload identity, not the admin pair.
        oo_stack::credentials::workload_identity(&root.join("pki"), "consumer")
            .map_err(Failure::local)?;
    }
    // D16: deployments provision the retained build chain for runtime builds.
    // OO_SHARE_NPM_REGISTRY: a registry that carries this exact CLI version,
    // named in the share prompt's install step (validated by share-prompt).
    let registry = std::env::var("OO_SHARE_NPM_REGISTRY").ok();
    let request = json!({"action":action,"root":root,"stack":stack,"image":image,"retained":true,"demo":demo,
    "prompt_npm_registry":registry,
    "templates":{
        "compose.yaml":include_str!("../../../../deploy/lite/compose.yaml"),
        "postgres.sh":include_str!("../../../../deploy/lite/postgres.sh"),
        "scheduler.py":include_str!("../../../../deploy/lite/scheduler.py"),
    },
    // Control-DB jobs schema for the scheduled cycle, armed after the first build.
    "jobs":jobs(),"cycle_events":include_str!("../../../../deploy/lite/cycle_events.sql")});
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

/// Control-DB jobs migrations for the scheduled retained cycle.
pub(super) fn jobs() -> Value {
    json!({
        "0000_bootstrap_roles.sql":include_str!("../../../oo-jobs/migrations/0000_bootstrap_roles.sql"),
        "0001_jobs_control_expand.sql":include_str!("../../../oo-jobs/migrations/0001_jobs_control_expand.sql"),
        "0002_event_project_binding_expand.sql":include_str!("../../../oo-jobs/migrations/0002_event_project_binding_expand.sql"),
        "0003_job_lease_operations_expand.sql":include_str!("../../../oo-jobs/migrations/0003_job_lease_operations_expand.sql"),
    })
}

/// The runtime image this installation is pinned to (empty when absent).
pub(super) fn pinned_image() -> String {
    std::env::var("OO_LITE_RUNTIME_IMAGE").unwrap_or_else(|_| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| fs::read_to_string(exe.with_file_name("oo-lite-image")).ok())
            .unwrap_or_default()
            .trim()
            .to_owned()
    })
}

fn saved_image(root: &Path) -> Option<String> {
    let owner: Value = serde_json::from_slice(&fs::read(root.join("owner.json")).ok()?).ok()?;
    owner["image"].as_str().map(str::to_owned)
}

/// The deployment's lane label; `OO_LITE_LANE` only applies before first up.
pub(super) fn lane(root: &Path) -> String {
    fs::read(root.join("owner.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|owner| owner["lane"].as_str().map(str::to_owned))
        .or_else(|| std::env::var("OO_LITE_LANE").ok())
        .filter(|l| {
            !l.is_empty()
                && l.len() <= 8
                && l.as_bytes()[0].is_ascii_lowercase()
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
        .unwrap_or_else(|| "a".into())
}

/// The runtime user: the owner of the private deployment directory, so
/// bind-mounted 0600 files stay readable without widening their modes.
pub(super) fn user(root: &Path) -> Result<String, Failure> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(root).map_err(|_| Failure::local("LITE_STATE_DIRECTORY"))?;
    Ok(format!("{}:{}", meta.uid(), meta.gid()))
}

/// Lite credentials use Ed25519, which macOS LibreSSL cannot create or read.
/// Without a capable host `openssl`, run OpenSSL from the pinned runtime image
/// (D16: no host helpers), seeing only the credential directory at its path.
pub(super) struct OpensslHelper(Option<String>);

impl OpensslHelper {
    pub(super) fn start(pki: &Path, image: &str) -> Result<Self, Failure> {
        Self::start_lane(pki, image, None)
    }

    fn start_lane(pki: &Path, image: &str, label: Option<&str>) -> Result<Self, Failure> {
        if oo_stack::credentials::host_openssl_supports_ed25519() {
            return Ok(Self(None));
        }
        match fs::DirBuilder::new().mode(0o700).create(pki) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(Failure::local("LITE_STATE_DIRECTORY")),
        }
        let pki = fs::canonicalize(pki).map_err(|_| Failure::local("LITE_STATE_DIRECTORY"))?;
        let root = pki
            .parent()
            .ok_or_else(|| Failure::local("LITE_STATE_DIRECTORY"))?;
        let name = format!("oo-lite-openssl-{}", std::process::id());
        let path = pki
            .to_str()
            .ok_or_else(|| Failure::local("LITE_STATE_DIRECTORY"))?;
        let mut command = Command::new("docker");
        command
            .args([
                "run",
                "--detach",
                "--rm",
                "--name",
                &name,
                "--network",
                "none",
            ])
            .args([
                "--label",
                &format!(
                    "oo.lane={}",
                    label.map(str::to_owned).unwrap_or_else(|| lane(root))
                ),
            ])
            .args(["--read-only", "--cap-drop", "ALL"])
            .args([
                "--security-opt",
                "no-new-privileges:true",
                "--user",
                &user(root)?,
            ])
            .args(["--volume", &format!("{path}:{path}")])
            .args(["--entrypoint", "sleep", image, "600"]);
        let (exit, _, _) = super::build_retained::invoke(command, vec![], 120)?;
        // Wait until the helper accepts exec before the first credential call.
        let ready = exit == 0
            && (0..40).any(|_| {
                let ok = Command::new("docker")
                    .args(["exec", &name, "openssl", "version"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
                if !ok {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                ok
            });
        if !ready
            || !oo_stack::credentials::use_openssl(
                ["docker", "exec", "-i", &name, "openssl"]
                    .map(str::to_owned)
                    .to_vec(),
            )
        {
            let _ = Command::new("docker").args(["rm", "-f", &name]).output();
            return Err(Failure::local("LITE_OPENSSL_RUNTIME"));
        }
        Ok(Self(Some(name)))
    }
}

impl Drop for OpensslHelper {
    fn drop(&mut self) {
        // Confirm removal; the helper's own short sleep bounds any leftover.
        if let Some(name) = &self.0 {
            for _ in 0..3 {
                let quiet = |args: &[&str]| {
                    Command::new("docker")
                        .args(args)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .is_ok_and(|s| s.success())
                };
                quiet(&["rm", "-f", name]);
                if !quiet(&["container", "inspect", name]) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
}

/// Whether `image` is an acceptable pinned content address.
pub(super) fn digest(image: &str) -> bool {
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

/// Pull a registry image pinned by digest; the content cannot differ from the
/// pin, so a fresh machine needs no manual step. Local image IDs are never
/// pulled.
fn pull(image: &str) -> Result<bool, Failure> {
    eprintln!("oo up: pulling {image}");
    let mut pull = Command::new("docker");
    pull.args(["pull", "--quiet", image]);
    let (exit, _, _) = super::build_retained::invoke(pull, vec![], 1800)?;
    if exit != 0 {
        return Ok(false);
    }
    let mut inspect = Command::new("docker");
    inspect.args(["image", "inspect", image]);
    let (exit, _, _) = super::build_retained::invoke(inspect, vec![], 30)?;
    Ok(exit == 0)
}

fn require_images(runtime: &str) -> Result<(), Failure> {
    let postgres = include_str!("../../../../deploy/lite/compose.yaml")
        .lines()
        .filter_map(|line| line.trim().strip_prefix("image: "))
        .find(|image| image.contains("postgres@sha256:"))
        .filter(|image| digest(image))
        .ok_or_else(|| Failure::local("LITE_COMPOSE_TEMPLATE"))?;
    let postgres_override = std::env::var("OO_LITE_POSTGRES_IMAGE").ok();
    let postgres = postgres_override.as_deref().unwrap_or(postgres);
    if !digest(postgres) {
        return Err(Failure::local("LITE_IMAGE_DIGEST_REQUIRED"));
    }
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
        if exit != 0 && !(image.contains('@') && pull(image)?) {
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
