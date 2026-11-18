//! The compose runtime of `oo serve` (10 §8.6 point 3 and §2.7, Decision D30).
//!
//! `oo serve --runtime compose` runs `docs-db`, `docs-engine` and `doc-parser`
//! as containers of the project `oo-docs-<hash>` described by
//! `deploy/docs/compose.yaml`. Startup order: `docker info`, image digest check
//! against `deploy/docs/images.lock.json`, state files and tokens, docs-db PKI,
//! generated override, `docs-db` up, one-shot schema install, runtime login,
//! `docs-engine` and `doc-parser` up, wait for `/livez`.
//!
//! The daemon never mounts the Docker socket into a container and never
//! publishes a port other than the engine's loopback one. Local Drives reach
//! the engine as read-only bind mounts `/drives/<id>` that this module adds to
//! the generated override; registering a Drive recreates the engine container
//! (10 §13: a few seconds of interruption, the proxy answers 503 meanwhile).
//!
//! Deviation from the spec text: the docs-db certificate authority is created
//! here (`<state>/pki`, ECDSA P-256, made by the `openssl` of the pinned
//! postgres image) and not by `oo_stack::credentials::bootstrap`, whose fixed
//! identity list is the lite stack's and would change what a lite
//! `credentials status` demands.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::proxy::DriveMounts;
use super::secrets;
use super::serve::exit;

/// Time the engine gets to answer `/livez` (10 §8.6 point 3).
pub const LIVEZ_TIMEOUT: Duration = Duration::from_secs(120);

/// Why the runtime did not come up, with the `oo serve` exit code to use.
#[derive(Debug)]
pub struct Failure {
    pub exit: u8,
    pub message: String,
}

fn fail<T>(exit: u8, message: impl Into<String>) -> Result<T, Failure> {
    Err(Failure {
        exit,
        message: message.into(),
    })
}

/// `oo-docs-<first 12 hex of sha256(canonical state directory)>`.
pub fn project_name(canonical_state: &Path) -> String {
    let digest = Sha256::digest(canonical_state.as_os_str().as_encoded_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("oo-docs-{}", &hex[..12])
}

/// `linux/arm64` or `linux/amd64` from `docker info`'s architecture.
pub fn platform_of(docker_arch: &str) -> Option<&'static str> {
    match docker_arch.trim() {
        "aarch64" | "arm64" => Some("linux/arm64"),
        "x86_64" | "amd64" => Some("linux/amd64"),
        _ => None,
    }
}

/// Image references the compose file uses, after the digest check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Images {
    pub engine: String,
    pub parser: String,
    pub db: String,
    /// Tags the lock records next to the ids; a tag that resolves to another
    /// image id means the local build is not the one the lock describes.
    pub engine_tag: Option<String>,
    pub parser_tag: Option<String>,
}

/// Verdict on one image: its id must be present and any recorded tag must
/// still resolve to that same id.
pub fn judge_image(
    name: &str,
    lock_id: &str,
    local_id: Option<&str>,
    tag: Option<(&str, Option<&str>)>,
) -> Result<(), String> {
    match local_id {
        None => {
            return Err(format!(
                "{name} image {lock_id} is not present; build it (see deploy/docs)"
            ));
        }
        Some(id) if id != lock_id => {
            return Err(format!(
                "{name} image digest mismatch: lock {lock_id}, local {id}"
            ));
        }
        Some(_) => {}
    }
    if let Some((tag, Some(resolved))) = tag
        && resolved != lock_id
    {
        return Err(format!(
            "{name} image digest mismatch: tag {tag} is {resolved}, lock {lock_id}"
        ));
    }
    Ok(())
}

/// Pure part of the image check: which references the lock wants for
/// `platform`, or why the lock cannot be used.
pub fn wanted_images(lock: &Value, platform: &str) -> Result<Images, String> {
    let entry = |name: &str| -> Result<String, String> {
        lock[name]["platforms"][platform]["image"]
            .as_str()
            .filter(|s| s.starts_with("sha256:") && s.len() == 71)
            .map(str::to_owned)
            .ok_or_else(|| format!("images.lock.json has no {name} image for {platform}"))
    };
    // The parser entry predates the engine entry and sits at the top level.
    let parser = lock["platforms"][platform]["image"]
        .as_str()
        .filter(|s| s.starts_with("sha256:") && s.len() == 71)
        .map(str::to_owned)
        .ok_or_else(|| format!("images.lock.json has no doc-parser image for {platform}"))?;
    let db = lock["base_indexes"]["postgres"]
        .as_str()
        .filter(|s| s.starts_with("sha256:"))
        .map(|d| format!("docker.io/library/postgres@{d}"))
        .ok_or("images.lock.json has no postgres digest")?;
    let tag = |v: &Value| v.as_str().map(str::to_owned);
    Ok(Images {
        engine: entry("engine")?,
        parser,
        db,
        engine_tag: tag(&lock["engine"]["platforms"][platform]["tag"]),
        parser_tag: tag(&lock["platforms"][platform]["tag"]),
    })
}

/// Free IPv4 /28 subnets for hosts whose default address pools are exhausted.
pub fn pick_subnets(used: &[(u32, u32)], count: usize) -> Vec<String> {
    let overlaps = |net: u32, prefix: u32| {
        used.iter().any(|&(n, p)| {
            let shift = 32 - p.min(prefix);
            (n >> shift) == (net >> shift)
        })
    };
    let mut out = Vec::new();
    for second in 0..250u32 {
        for block in 0..16u32 {
            let net = (10 << 24) | (249 << 16) | (second << 8) | (block << 4);
            if !overlaps(net, 28) {
                out.push(format!(
                    "{}.{}.{}.{}/28",
                    net >> 24,
                    (net >> 16) & 255,
                    (net >> 8) & 255,
                    net & 255
                ));
                if out.len() == count {
                    return out;
                }
            }
        }
    }
    out
}

/// `a.b.c.d/n` as (network, prefix).
pub fn parse_cidr(text: &str) -> Option<(u32, u32)> {
    let (addr, prefix) = text.split_once('/')?;
    let parts: Vec<u32> = addr
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [a, b, c, d] = parts[..] else { return None };
    if parts.iter().any(|p| *p > 255) {
        return None;
    }
    let prefix: u32 = prefix.parse().ok().filter(|p| *p <= 32)?;
    Some(((a << 24) | (b << 16) | (c << 8) | d, prefix))
}

/// Mount id of a canonical host path: first 16 hex of its sha256.
pub fn mount_id(canonical: &Path) -> String {
    let digest = Sha256::digest(canonical.as_os_str().as_encoded_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn run(cmd: &mut Command) -> Result<Output, String> {
    cmd.stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {:?}: {e}", cmd.get_program()))
}

fn ok_stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn tail(out: &Output) -> String {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<&str> = text.lines().rev().take(8).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join(" | ")
}

/// The running compose project.
pub struct Runtime {
    deploy: PathBuf,
    state: PathBuf,
    project: String,
    lane: String,
    images: Images,
    uid: u32,
    gid: u32,
    pub engine_port: u16,
    db_runtime_password: String,
    db_admin_password: String,
    mounts: Mutex<BTreeMap<String, PathBuf>>,
    /// Model secret files bound read-only into the engine at their own path.
    secret_files: Mutex<BTreeSet<PathBuf>>,
    subnets: Mutex<Option<Vec<String>>>,
}

impl Runtime {
    fn home(&self) -> PathBuf {
        self.state.join("home")
    }
    fn pki(&self) -> PathBuf {
        self.state.join("pki")
    }
    fn override_file(&self) -> PathBuf {
        self.state.join("compose.override.yaml")
    }

    /// Engine address on the host.
    pub fn engine_addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.engine_port))
    }

    /// What `serve.json` reports under `engine`.
    pub fn describe(&self) -> Value {
        json!({
            "mode": "compose",
            "project": self.project,
            "port": self.engine_port,
            "images": {
                "docs-engine": self.images.engine,
                "doc-parser": self.images.parser,
                "docs-db": self.images.db,
            },
        })
    }

    fn db_url(&self, user: &str, password: &str) -> String {
        format!(
            "postgres://{user}:{password}@docs-db:5432/oo_docs?sslmode=verify-full&sslrootcert=/run/oo/ca.crt"
        )
    }

    /// The override file: DB logins, allowed hosts, Drive mounts, subnets.
    pub fn override_json(&self) -> Value {
        let mounts = self.mounts.lock().unwrap_or_else(|p| p.into_inner());
        let binds: Vec<Value> = mounts
            .iter()
            .map(|(id, host)| {
                json!({
                    "type": "bind",
                    "source": host,
                    "target": format!("/drives/{id}"),
                    "read_only": true,
                    "bind": {"create_host_path": false},
                })
            })
            .collect();
        let mut binds = binds;
        binds.extend(self.secret_file_binds());
        let port = self.engine_port;
        let mut doc = json!({
            "services": {
                "docs-engine": {
                    "environment": {
                        "OO_DOCS_DATABASE_URL": self.db_url("oo_docs_login", &self.db_runtime_password),
                        "OO_DOCS_ALLOWED_HOSTS": format!("127.0.0.1:{port},localhost:{port}"),
                    },
                    "volumes": binds,
                },
                "docs-installer": {
                    "environment": {
                        "OO_DOCS_DATABASE_URL": self.db_url("postgres", &self.db_admin_password),
                        "OO_DOCS_ADMIN_DATABASE_URL": self.db_url("postgres", &self.db_admin_password),
                    },
                },
            }
        });
        if let Some(subnets) = self
            .subnets
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            doc["networks"] = json!({
                "docs-internal": {"ipam": {"config": [{"subnet": subnets[0]}]}},
                "docs-egress": {"ipam": {"config": [{"subnet": subnets[1]}]}},
            });
        }
        doc
    }

    /// `file:` references of the saved model configuration (`<state>/engine/model.json`).
    fn saved_secret_files(&self) -> Vec<PathBuf> {
        let Ok(text) = fs::read_to_string(self.state.join("engine/model.json")) else {
            return Vec::new();
        };
        super::proxy::model_secret_files(text.as_bytes())
            .into_iter()
            .map(PathBuf::from)
            .collect()
    }

    /// Read-only binds of the model's secret files, at the same absolute path
    /// inside the engine: the egress process there reads them itself (a `file:`
    /// reference stays valid, the engine never sees a value). Only existing
    /// regular files are bound; the engine's own refusal reports the rest.
    fn secret_file_binds(&self) -> Vec<Value> {
        let mut files: BTreeSet<PathBuf> = self.saved_secret_files().into_iter().collect();
        files.extend(
            self.secret_files
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .cloned(),
        );
        files
            .into_iter()
            .filter(|p| p.is_absolute() && fs::metadata(p).is_ok_and(|m| m.is_file()))
            .map(|p| {
                json!({
                    "type": "bind",
                    "source": p,
                    "target": p,
                    "read_only": true,
                    "bind": {"create_host_path": false},
                })
            })
            .collect()
    }

    fn write_override(&self) -> Result<(), Failure> {
        let text = serde_json::to_string_pretty(&self.override_json()).unwrap_or_default();
        secrets::write_secret_atomic(&self.override_file(), &text)
            .or_else(|_| fail(exit::OTHER, "cannot write compose.override.yaml"))
    }

    fn compose(&self) -> Command {
        let mut c = Command::new("docker");
        c.args([
            "compose",
            "-p",
            &self.project,
            "--project-directory",
            &self.deploy.to_string_lossy(),
            "-f",
            &self.deploy.join("compose.yaml").to_string_lossy(),
            "-f",
            &self.override_file().to_string_lossy(),
        ])
        .env("OO_DOCS_PROJECT", &self.project)
        .env("OO_DOCS_LANE", &self.lane)
        .env("OO_DOCS_STATE", self.home())
        .env("OO_DOCS_ENGINE_STATE", self.state.join("engine"))
        .env("OO_DOCS_PKI", self.pki())
        .env("OO_DOCS_UID", self.uid.to_string())
        .env("OO_DOCS_GID", self.gid.to_string())
        .env("OO_DOCS_ENGINE_PORT", self.engine_port.to_string())
        .env("OO_DOCS_ENGINE_IMAGE", &self.images.engine)
        .env("OO_DOCS_PARSER_IMAGE", &self.images.parser)
        .env("OO_DOCS_DB_IMAGE", &self.images.db);
        c
    }

    /// Run a compose subcommand; on exhausted address pools pin subnets once and retry.
    fn compose_run(&self, args: &[&str]) -> Result<Output, String> {
        let attempt = |me: &Self| -> Result<Output, String> {
            let mut c = me.compose();
            c.args(args);
            run(&mut c)
        };
        let out = attempt(self)?;
        if !out.status.success() && tail(&out).contains("predefined address pools") {
            let used = used_subnets();
            let picked = pick_subnets(&used, 2);
            if picked.len() == 2 {
                // Remembered across starts: a different network definition on
                // the next start would make compose recreate the networks
                // under the existing containers.
                let doc = json!({"protocol": "oo.docs.subnets/v1", "subnets": picked});
                let _ = secrets::write_secret_atomic(
                    &self.state.join("subnets.json"),
                    &doc.to_string(),
                );
                *self.subnets.lock().unwrap_or_else(|p| p.into_inner()) = Some(picked);
                self.write_override().map_err(|f| f.message)?;
                return attempt(self);
            }
        }
        Ok(out)
    }

    fn compose_ok(&self, args: &[&str], what: &str) -> Result<(), Failure> {
        match self.compose_run(args) {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => fail(exit::OTHER, format!("{what} failed: {}", tail(&o))),
            Err(e) => fail(exit::OTHER, format!("{what} failed: {e}")),
        }
    }

    fn engine_running(&self) -> bool {
        self.compose_run(&["ps", "-q", "--status", "running", "docs-engine"])
            .is_ok_and(|o| o.status.success() && !ok_stdout(&o).is_empty())
    }

    /// Wait until the engine answers `/livez` on its loopback port.
    pub fn wait_livez(&self, within: Duration) -> Result<(), Failure> {
        let until = Instant::now() + within;
        while Instant::now() < until {
            if livez(self.engine_addr()) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        fail(
            exit::ENGINE_TIMEOUT,
            "the docs engine did not answer /livez in time",
        )
    }

    /// Bring everything up (steps 4 to 10 of §8.6 point 3 after the checks).
    pub fn up(&self) -> Result<(), Failure> {
        self.write_override()?;
        self.compose_ok(
            &["up", "-d", "--wait", "--wait-timeout", "120", "docs-db"],
            "docker compose up docs-db",
        )?;
        self.install_schema()?;

        self.ensure_login()?;
        let was_running = self.engine_running();
        self.compose_ok(
            &[
                "up",
                "-d",
                "--wait",
                "--wait-timeout",
                "120",
                "docs-engine",
                "doc-parser",
            ],
            "docker compose up docs-engine doc-parser",
        )?;
        if was_running {
            // A reused engine read its token files at its own start; the
            // daemon rotated them, so the engine restarts (the databases and
            // the parser stay up).
            self.compose_ok(&["restart", "-t", "20", "docs-engine"], "engine restart")?;
        }
        self.wait_livez(LIVEZ_TIMEOUT)
    }

    /// The one-shot schema install. It is idempotent, and the first connection
    /// of a freshly started database sometimes loses the race with the
    /// network's name registration, so it is retried a few times.
    fn install_schema(&self) -> Result<(), Failure> {
        let args = [
            "--profile",
            "install",
            "run",
            "--rm",
            "--no-deps",
            "-T",
            "docs-installer",
        ];
        let mut last = String::new();
        for attempt in 0..5 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(3));
            }
            match self.compose_run(&args) {
                Ok(o) if o.status.success() => return Ok(()),
                Ok(o) => last = tail(&o),
                Err(e) => last = e,
            }
        }
        fail(exit::OTHER, format!("schema install failed: {last}"))
    }

    /// Create or refresh the `oo_docs_login` role (member of `oo_docs_runtime`).
    fn ensure_login(&self) -> Result<(), Failure> {
        let sql = format!(
            "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'oo_docs_login') THEN \
             CREATE ROLE oo_docs_login LOGIN IN ROLE oo_docs_runtime; END IF; END $$;\n\
             ALTER ROLE oo_docs_login PASSWORD '{}';\n",
            self.db_runtime_password
        );
        let mut c = self.compose();
        c.args([
            "exec",
            "-T",
            "docs-db",
            "psql",
            "-U",
            "postgres",
            "-d",
            "oo_docs",
            "-v",
            "ON_ERROR_STOP=1",
            "-q",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
        let mut child = c
            .spawn()
            .or_else(|e| fail(exit::OTHER, format!("psql did not start: {e}")))?;
        // The password goes over stdin, never in an argument list.
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(sql.as_bytes());
        }
        let out = child
            .wait_with_output()
            .or_else(|e| fail(exit::OTHER, format!("psql failed: {e}")))?;
        if out.status.success() {
            Ok(())
        } else {
            fail(
                exit::OTHER,
                format!(
                    "creating the runtime login failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                        .lines()
                        .last()
                        .unwrap_or("")
                ),
            )
        }
    }

    /// `docker compose stop` (volumes stay), unless the engine is to be kept.
    pub fn stop(&self, keep_engine: bool) {
        if keep_engine {
            return;
        }
        let _ = self.compose_run(&["stop", "-t", "20"]);
    }

    fn save_mounts(&self, map: &BTreeMap<String, PathBuf>) {
        let doc = json!({"protocol": "oo.docs.mounts/v1", "mounts": map});
        let _ = secrets::write_secret_atomic(&self.state.join("mounts.json"), &doc.to_string());
    }

    fn mount_checked(&self, host_root: &str) -> Result<String, String> {
        let canonical = fs::canonicalize(host_root)
            .map_err(|_| "source root is missing or not a directory".to_owned())?;
        if !canonical.is_dir() {
            return Err("source root is missing or not a directory".into());
        }
        if canonical == Path::new("/")
            || canonical.starts_with(&self.state)
            || self.state.starts_with(&canonical)
        {
            return Err("this directory cannot be mounted into the engine".into());
        }
        let id = mount_id(&canonical);
        let target = format!("/drives/{id}");
        let mut map = self.mounts.lock().unwrap_or_else(|p| p.into_inner());
        if map.contains_key(&id) {
            return Ok(target);
        }
        map.insert(id, canonical);
        let snapshot = map.clone();
        drop(map);
        self.save_mounts(&snapshot);
        self.write_override().map_err(|f| f.message)?;
        // Recreate the engine with the new bind.
        let out = self
            .compose_run(&[
                "up",
                "-d",
                "--no-deps",
                "--wait",
                "--wait-timeout",
                "120",
                "docs-engine",
            ])
            .map_err(|e| format!("engine recreate failed: {e}"))?;
        if !out.status.success() {
            return Err(format!("engine recreate failed: {}", tail(&out)));
        }
        self.wait_livez(LIVEZ_TIMEOUT).map_err(|f| f.message)?;
        Ok(target)
    }
}

impl DriveMounts for Runtime {
    fn mount(&self, host_root: &str) -> Result<String, String> {
        self.mount_checked(host_root)
    }

    fn mount_secret_files(&self, files: &[String]) -> Result<(), String> {
        let wanted: BTreeSet<PathBuf> = files
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && fs::metadata(p).is_ok_and(|m| m.is_file()))
            .collect();
        let known: BTreeSet<PathBuf> = {
            let mut k: BTreeSet<PathBuf> = self.saved_secret_files().into_iter().collect();
            k.extend(
                self.secret_files
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .iter()
                    .cloned(),
            );
            k
        };
        if wanted.is_subset(&known) {
            return Ok(());
        }
        self.secret_files
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .extend(wanted);
        self.write_override().map_err(|f| f.message)?;
        // Recreate the engine with the new binds (the same step as a new Drive).
        let out = self
            .compose_run(&[
                "up",
                "-d",
                "--no-deps",
                "--wait",
                "--wait-timeout",
                "120",
                "docs-engine",
            ])
            .map_err(|e| format!("engine recreate failed: {e}"))?;
        if !out.status.success() {
            return Err(format!("engine recreate failed: {}", tail(&out)));
        }
        self.wait_livez(LIVEZ_TIMEOUT).map_err(|f| f.message)
    }
}

/// `GET /livez` over plain HTTP on the loopback port.
pub fn livez(addr: SocketAddr) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let req = format!("GET /livez HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    let n = s.read(&mut buf).unwrap_or(0);
    buf[..n].starts_with(b"HTTP/1.1 2")
}

fn used_subnets() -> Vec<(u32, u32)> {
    let Ok(ids) = run(Command::new("docker").args(["network", "ls", "-q"])) else {
        return Vec::new();
    };
    let ids: Vec<String> = ok_stdout(&ids)
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let Ok(out) = run(Command::new("docker")
        .args(["network", "inspect", "--format"])
        .arg("{{range .IPAM.Config}}{{.Subnet}} {{end}}")
        .args(&ids))
    else {
        return Vec::new();
    };
    ok_stdout(&out)
        .split_whitespace()
        .filter_map(parse_cidr)
        .collect()
}

fn free_loopback_port() -> Option<u16> {
    let l = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    l.local_addr().ok().map(|a| a.port())
}

/// Find `deploy/docs`: `--deploy-dir`, `OO_DOCS_DEPLOY_DIR`, the working
/// directory's `deploy/docs`, then the ancestors of this executable.
pub fn find_deploy_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    let ok = |p: &Path| p.join("compose.yaml").is_file() && p.join("images.lock.json").is_file();
    if let Some(p) = explicit {
        return ok(p).then(|| p.to_path_buf());
    }
    if let Some(p) = std::env::var_os("OO_DOCS_DEPLOY_DIR").map(PathBuf::from) {
        return ok(&p).then_some(p);
    }
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.extend(cwd.ancestors().map(|a| a.join("deploy/docs")));
    }
    if let Ok(exe) = std::env::current_exe() {
        candidates.extend(exe.ancestors().map(|a| a.join("deploy/docs")));
    }
    candidates.into_iter().find(|p| ok(p))
}

fn image_id(reference: &str) -> Option<String> {
    let out =
        run(Command::new("docker").args(["image", "inspect", "--format", "{{.Id}}", reference]))
            .ok()?;
    out.status.success().then(|| ok_stdout(&out))
}

/// Everything before `up`: Docker, images, files, tokens, PKI.
pub fn prepare(
    state: &Path,
    deploy_dir: Option<&Path>,
    engine_token: &str,
    parser_token: &str,
) -> Result<Runtime, Failure> {
    // 1. docker info
    let info = run(Command::new("docker").args(["info", "--format", "{{.Architecture}}"]))
        .ok()
        .filter(|o| o.status.success())
        .map(|o| ok_stdout(&o));
    let Some(arch) = info else {
        return fail(exit::DOCKER, "Docker is not available (docker info failed)");
    };
    let compose_ok =
        run(Command::new("docker").args(["compose", "version"])).is_ok_and(|o| o.status.success());
    if !compose_ok {
        return fail(exit::DOCKER, "docker compose is not available");
    }
    let Some(platform) = platform_of(&arch) else {
        return fail(
            exit::ENGINE_MISSING,
            format!("unsupported Docker architecture {arch}"),
        );
    };

    // 2. image digests
    let Some(deploy) = find_deploy_dir(deploy_dir) else {
        return fail(
            exit::ENGINE_MISSING,
            "deploy/docs was not found; set --deploy-dir or OO_DOCS_DEPLOY_DIR",
        );
    };
    let lock: Value = fs::read_to_string(deploy.join("images.lock.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or_else(|| Failure {
            exit: exit::ENGINE_MISSING,
            message: "images.lock.json is unreadable".into(),
        })?;
    let images = wanted_images(&lock, platform).map_err(|m| Failure {
        exit: exit::ENGINE_MISSING,
        message: m,
    })?;
    for (name, reference, tag) in [
        ("docs-engine", &images.engine, &images.engine_tag),
        ("doc-parser", &images.parser, &images.parser_tag),
    ] {
        let local = image_id(reference);
        let by_tag = tag.as_deref().map(|t| (t, image_id(t)));
        let by_tag_ref = by_tag.as_ref().map(|(t, id)| (*t, id.as_deref()));
        judge_image(name, reference, local.as_deref(), by_tag_ref)
            .or_else(|m| fail(exit::ENGINE_MISSING, m))?;
    }
    if image_id(&images.db).is_none() {
        let pulled = run(Command::new("docker").args(["pull", "--quiet", &images.db]))
            .is_ok_and(|o| o.status.success());
        if !pulled {
            return fail(
                exit::ENGINE_MISSING,
                format!("postgres image {} is not present", images.db),
            );
        }
    }

    // 3. state files
    let canonical = fs::canonicalize(state)
        .or_else(|e| fail(exit::STATE_DIR, format!("state directory: {e}")))?;
    let meta = fs::metadata(&canonical)
        .or_else(|e| fail(exit::STATE_DIR, format!("state directory: {e}")))?;
    let (uid, gid) = (meta.uid(), meta.gid());
    for sub in ["home", "engine", "pki"] {
        secrets::ensure_state_dir(&canonical.join(sub))
            .or_else(|e| fail(exit::STATE_DIR, format!("{sub}: {}", e.0)))?;
    }
    let home = canonical.join("home");
    for (name, token) in [
        ("engine-token", engine_token),
        ("parser-token", parser_token),
    ] {
        secrets::write_secret_file(&home.join(name), token)
            .or_else(|_| fail(exit::STATE_DIR, format!("cannot write {name}")))?;
    }
    let lane = std::env::var("OO_DOCS_LANE").unwrap_or_else(|_| "docs".into());

    // 4. docs-db PKI and passwords
    let pki = canonical.join("pki");
    let (admin_pw, runtime_pw) = ensure_pki(&pki, &images.db, uid, gid)?;

    let mounts = read_mounts(&canonical);
    let subnets = read_subnets(&canonical);
    let Some(engine_port) = free_loopback_port() else {
        return fail(exit::OTHER, "no free loopback port");
    };
    Ok(Runtime {
        deploy,
        project: project_name(&canonical),
        state: canonical,
        lane,
        images,
        uid,
        gid,
        engine_port,
        db_runtime_password: runtime_pw,
        db_admin_password: admin_pw,
        mounts: Mutex::new(mounts),
        secret_files: Mutex::new(BTreeSet::new()),
        subnets: Mutex::new(subnets),
    })
}

fn read_subnets(state: &Path) -> Option<Vec<String>> {
    let v: Value =
        serde_json::from_str(&fs::read_to_string(state.join("subnets.json")).ok()?).ok()?;
    let list: Vec<String> = v["subnets"]
        .as_array()?
        .iter()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect();
    (list.len() == 2 && list.iter().all(|s| parse_cidr(s).is_some())).then_some(list)
}

fn read_mounts(state: &Path) -> BTreeMap<String, PathBuf> {
    let Some(v) = fs::read_to_string(state.join("mounts.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return BTreeMap::new();
    };
    v["mounts"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, p)| {
                    let path = PathBuf::from(p.as_str()?);
                    path.is_dir().then(|| (k.clone(), path))
                })
                .collect()
        })
        .unwrap_or_default()
}

const PKI_SCRIPT: &str = r#"set -eu
cd /pki
umask 077
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out ca-key
openssl req -new -x509 -key ca-key -days 730 -subj "/CN=Open Ontology docs internal CA" \
  -addext "basicConstraints=critical,CA:TRUE,pathlen:0" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -addext "subjectKeyIdentifier=hash" -out ca-cert
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out docs-db-key
openssl req -new -key docs-db-key -subj "/CN=docs-db" -out docs-db.csr
printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\nsubjectAltName=DNS:docs-db,DNS:localhost\n' > docs-db.ext
openssl x509 -req -in docs-db.csr -CA ca-cert -CAkey ca-key -set_serial "0x$(openssl rand -hex 16)" -days 730 -extfile docs-db.ext -out docs-db-cert
rm -f docs-db.csr docs-db.ext
"#;

/// Create the PKI once (never rotated while valid for 14 more days) and the two
/// database passwords; returns (admin, runtime) passwords.
fn ensure_pki(pki: &Path, db_image: &str, uid: u32, gid: u32) -> Result<(String, String), Failure> {
    let have = [
        "ca-cert",
        "docs-db-cert",
        "docs-db-key",
        "admin-password",
        "runtime-password",
    ]
    .iter()
    .all(|n| pki.join(n).is_file());
    let fresh = have && {
        let out = run(Command::new("docker")
            .args(["run", "--rm", "--network", "none", "--user"])
            .arg(format!("{uid}:{gid}"))
            .arg("--entrypoint")
            .arg("openssl")
            .args(["-v"])
            .arg(format!("{}:/pki:ro", pki.display()))
            .args([
                db_image,
                "x509",
                "-in",
                "/pki/docs-db-cert",
                "-noout",
                "-checkend",
                "1209600",
            ]));
        out.is_ok_and(|o| o.status.success())
    };
    if !fresh {
        for n in ["ca-key", "ca-cert", "docs-db-key", "docs-db-cert"] {
            let _ = fs::remove_file(pki.join(n));
        }
        let out = run(Command::new("docker")
            .args(["run", "--rm", "--network", "none", "--user"])
            .arg(format!("{uid}:{gid}"))
            .args(["--entrypoint", "sh", "-v"])
            .arg(format!("{}:/pki", pki.display()))
            .args([db_image, "-c", PKI_SCRIPT]))
        .or_else(|e| fail(exit::OTHER, e))?;
        if !out.status.success() {
            return fail(
                exit::OTHER,
                format!("docs-db PKI generation failed: {}", tail(&out)),
            );
        }
    }
    for n in ["admin-password", "runtime-password"] {
        if !pki.join(n).is_file() {
            let pw = secrets::new_token("")
                .or_else(|_| fail(exit::OTHER, "no randomness source"))?
                .replace(['-', '_'], "x");
            secrets::write_secret_file(&pki.join(n), &pw)
                .or_else(|_| fail(exit::STATE_DIR, format!("cannot write {n}")))?;
        }
    }
    let read = |n: &str| {
        fs::read_to_string(pki.join(n))
            .map(|s| s.trim().to_owned())
            .or_else(|_| fail(exit::STATE_DIR, format!("cannot read {n}")))
    };
    Ok((read("admin-password")?, read("runtime-password")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock() -> Value {
        let id = |c: char| format!("sha256:{}", c.to_string().repeat(64));
        json!({
            "base_indexes": {"postgres": id('d')},
            "platforms": {"linux/arm64": {"image": id('a')}, "linux/amd64": null},
            "engine": {"platforms": {"linux/arm64": {"image": id('b'), "tag": "oo-docs-engine:dev"}, "linux/amd64": null}},
        })
    }

    #[test]
    fn project_name_is_twelve_hex_of_the_state_path() {
        let a = project_name(Path::new("/state/a"));
        assert!(a.starts_with("oo-docs-") && a.len() == "oo-docs-".len() + 12);
        assert_ne!(a, project_name(Path::new("/state/b")));
        assert_eq!(a, project_name(Path::new("/state/a")));
    }

    #[test]
    fn lock_resolution_needs_both_images_and_the_postgres_digest() {
        let w = wanted_images(&lock(), "linux/arm64").unwrap();
        assert!(w.engine.ends_with(&"b".repeat(64)) && w.parser.ends_with(&"a".repeat(64)));
        assert_eq!(
            w.db,
            format!("docker.io/library/postgres@sha256:{}", "d".repeat(64))
        );
        let err = wanted_images(&lock(), "linux/amd64").unwrap_err();
        assert!(err.contains("amd64"), "{err}");
        let mut no_engine = lock();
        no_engine.as_object_mut().unwrap().remove("engine");
        assert!(
            wanted_images(&no_engine, "linux/arm64")
                .unwrap_err()
                .contains("engine")
        );
    }

    #[test]
    fn images_are_judged_by_id_and_by_the_tag_the_lock_records() {
        let want = format!("sha256:{}", "b".repeat(64));
        let other = format!("sha256:{}", "c".repeat(64));
        assert!(judge_image("e", &want, Some(&want), Some(("t", Some(&want)))).is_ok());
        assert!(judge_image("e", &want, Some(&want), Some(("t", None))).is_ok());
        assert!(judge_image("e", &want, Some(&want), None).is_ok());
        let missing = judge_image("e", &want, None, None).unwrap_err();
        assert!(missing.contains("not present"), "{missing}");
        let moved = judge_image("e", &want, Some(&want), Some(("t", Some(&other)))).unwrap_err();
        assert!(
            moved.contains("mismatch") && moved.contains("tag t"),
            "{moved}"
        );
        let wrong = judge_image("e", &want, Some(&other), None).unwrap_err();
        assert!(wrong.contains("mismatch"), "{wrong}");
        let w = wanted_images(&lock(), "linux/arm64").unwrap();
        assert_eq!(w.engine_tag.as_deref(), Some("oo-docs-engine:dev"));
        assert_eq!(w.parser_tag, None);
    }

    #[test]
    fn architectures_map_to_platforms() {
        assert_eq!(platform_of("aarch64"), Some("linux/arm64"));
        assert_eq!(platform_of("x86_64\n"), Some("linux/amd64"));
        assert_eq!(platform_of("riscv64"), None);
    }

    #[test]
    fn mount_ids_are_stable_and_distinct() {
        let a = mount_id(Path::new("/drives/a"));
        assert_eq!(a.len(), 16);
        assert_eq!(a, mount_id(Path::new("/drives/a")));
        assert_ne!(a, mount_id(Path::new("/drives/b")));
    }

    #[test]
    fn subnets_skip_what_is_in_use() {
        let used = [
            parse_cidr("10.249.0.0/24").unwrap(),
            parse_cidr("10.249.1.16/28").unwrap(),
        ];
        let got = pick_subnets(&used, 2);
        assert_eq!(got, ["10.249.1.0/28", "10.249.1.32/28"]);
        assert_eq!(parse_cidr("10.0.0.0/33"), None);
        assert_eq!(parse_cidr("999.0.0.0/8"), None);
    }

    fn runtime_for_test(mounts: &[(&str, &str)]) -> Runtime {
        Runtime {
            deploy: "/d".into(),
            state: "/s".into(),
            project: "oo-docs-x".into(),
            lane: "docs".into(),
            images: wanted_images(&lock(), "linux/arm64").unwrap(),
            uid: 501,
            gid: 20,
            engine_port: 18480,
            db_runtime_password: "rt".into(),
            db_admin_password: "adm".into(),
            mounts: Mutex::new(
                mounts
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), PathBuf::from(v)))
                    .collect(),
            ),
            secret_files: Mutex::new(BTreeSet::new()),
            subnets: Mutex::new(None),
        }
    }

    #[test]
    fn the_override_carries_logins_hosts_and_read_only_drive_binds() {
        let rt = runtime_for_test(&[("0123456789abcdef", "/data/drive")]);
        let o = rt.override_json();
        let env = &o["services"]["docs-engine"]["environment"];
        assert_eq!(
            env["OO_DOCS_ALLOWED_HOSTS"],
            "127.0.0.1:18480,localhost:18480"
        );
        let url = env["OO_DOCS_DATABASE_URL"].as_str().unwrap();
        assert!(
            url.starts_with("postgres://oo_docs_login:rt@docs-db:5432/oo_docs?sslmode=verify-full")
        );
        let bind = &o["services"]["docs-engine"]["volumes"][0];
        assert_eq!(bind["target"], "/drives/0123456789abcdef");
        assert_eq!(bind["read_only"], true);
        assert_eq!(bind["source"], "/data/drive");
        // the installer uses the admin login; the long-running engine never gets it
        let inst = &o["services"]["docs-installer"]["environment"];
        assert!(
            inst["OO_DOCS_ADMIN_DATABASE_URL"]
                .as_str()
                .unwrap()
                .contains("postgres:adm@")
        );
        assert!(!env.to_string().contains("adm"));
        assert!(o.get("networks").is_none());
    }

    #[test]
    fn model_secret_files_are_bound_read_only_at_their_own_path() {
        let dir = std::env::temp_dir().join(format!("oo-compose-secret-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("key");
        std::fs::write(&key, b"x").unwrap();
        let rt = runtime_for_test(&[]);
        // Nothing configured: no extra bind.
        assert!(rt.secret_file_binds().is_empty());
        rt.secret_files.lock().unwrap().extend([
            key.clone(),
            dir.join("missing"),
            PathBuf::from("relative"),
        ]);
        let binds = rt.secret_file_binds();
        assert_eq!(binds.len(), 1, "only an existing absolute file");
        assert_eq!(binds[0]["source"], binds[0]["target"]);
        assert_eq!(binds[0]["read_only"], true);
        assert_eq!(binds[0]["bind"]["create_host_path"], false);
        let o = rt.override_json();
        let vols = o["services"]["docs-engine"]["volumes"].as_array().unwrap();
        assert!(vols.iter().any(|v| v["target"] == key.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pinned_subnets_are_remembered_across_starts() {
        let dir = std::env::temp_dir().join(format!("oo-compose-subnets-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_subnets(&dir), None);
        fs::write(
            dir.join("subnets.json"),
            r#"{"subnets":["10.249.0.0/28","10.249.0.16/28"]}"#,
        )
        .unwrap();
        assert_eq!(
            read_subnets(&dir),
            Some(vec!["10.249.0.0/28".into(), "10.249.0.16/28".into()])
        );
        fs::write(dir.join("subnets.json"), r#"{"subnets":["junk"]}"#).unwrap();
        assert_eq!(read_subnets(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pinned_subnets_appear_in_the_override() {
        let rt = runtime_for_test(&[]);
        *rt.subnets.lock().unwrap() = Some(vec!["10.249.0.0/28".into(), "10.249.0.16/28".into()]);
        let o = rt.override_json();
        assert_eq!(
            o["networks"]["docs-internal"]["ipam"]["config"][0]["subnet"],
            "10.249.0.0/28"
        );
    }

    #[test]
    fn mounting_refuses_the_root_the_state_dir_and_missing_paths() {
        let rt = runtime_for_test(&[]);
        assert!(rt.mount_checked("/").is_err());
        assert!(rt.mount_checked("/definitely/not/here").is_err());
        assert!(rt.mount_checked("/s").is_err());
    }
}
