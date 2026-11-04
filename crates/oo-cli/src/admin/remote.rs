//! `oo up|down|status --target ssh`: the lite compose stack on a remote Linux
//! host through the system OpenSSH client.
//!
//! The host key is pinned: `ssh-keyscan` output is reduced to the single key
//! whose SHA256 fingerprint equals `deploy.ssh.host_key_sha256`, written to a
//! private known_hosts file, and every `ssh` runs with
//! `StrictHostKeyChecking=yes` against only that file. User SSH configuration,
//! agents and forwarding are disabled. Secrets travel only on the SSH stdin
//! stream to `deploy/lite/remote.py`; passwords reach `ssh` through this binary
//! as `SSH_ASKPASS` reading the private secret file, never through argv or the
//! environment. Diagnostics carry codes, not remote output.
use super::{Failure, Invocation};
use oo_stack::stack::{DeploySsh, ModelEndpoint, Stack, interval_seconds};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{ChildStdout, Command, Stdio};

const AGENT: &str = include_str!("../../../../deploy/lite/remote.py");
const LIFECYCLE: &str = include_str!("../../../../deploy/lite/lifecycle.py");
const COMPOSE: &str = include_str!("../../../../deploy/lite/compose.yaml");
const POSTGRES_SH: &str = include_str!("../../../../deploy/lite/postgres.sh");
const SCHEDULER: &str = include_str!("../../../../deploy/lite/scheduler.py");
const PROTOCOL: &str = "oo.lite-remote/v1";
const ASKPASS: &str = "OO_SSH_ASKPASS_SECRET";
/// Reads exactly the agent bytes from stdin, then hands the rest of the stream
/// (one JSON line, optionally an image archive) to the agent. No secrets.
const LOADER: &str = "python3 -c 'import sys\nr=sys.stdin.buffer.raw\nn=int(r.readline())\nb=bytearray()\nwhile len(b)<n:\n c=r.read(n-len(b))\n if not c:raise SystemExit(97)\n b+=c\nexec(compile(bytes(b),\"oo-remote\",\"exec\"))'";

fn fail(code: &str) -> Failure {
    Failure::local(code.to_owned())
}

/// `SSH_ASKPASS` mode: print one password secret for an ssh password prompt.
pub(crate) fn askpass(args: &[String]) -> Option<Result<Invocation, Failure>> {
    let name = std::env::var(ASKPASS).ok()?;
    if args.len() != 1 || !args[0].to_ascii_lowercase().contains("password") {
        return Some(Err(fail("SSH_ASKPASS_PROMPT")));
    }
    Some(
        oo_stack::secret::SecretStore::from_home()
            .and_then(|store| store.read(&name))
            .map(|secret| Invocation::Help(secret.trim_end_matches(['\r', '\n']).to_owned()))
            .map_err(Failure::local),
    )
}

struct Remote {
    options: Vec<OsString>,
    host: String,
    password: Option<String>,
}

impl Remote {
    fn command(&self) -> Command {
        let mut command = Command::new("ssh");
        command
            .args(&self.options)
            .arg("--")
            .arg(&self.host)
            .arg(LOADER);
        if let Some(name) = &self.password {
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("oo"));
            command
                .env("SSH_ASKPASS", exe)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env("DISPLAY", "oo:0")
                .env(ASKPASS, name);
        } else {
            command.env_remove(ASKPASS);
        }
        command
    }

    /// argv and environment for one fixed remote command over the pinned
    /// connection (no agent, no secrets in either).
    fn plain(&self, command: &str) -> (Vec<String>, Vec<(String, String)>) {
        let mut argv = vec!["ssh".to_owned()];
        argv.extend(
            self.options
                .iter()
                .map(|o| o.to_string_lossy().into_owned()),
        );
        argv.extend(["--".to_owned(), self.host.clone(), command.to_owned()]);
        let mut env = Vec::new();
        if let Some(name) = &self.password {
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("oo"));
            env = vec![
                ("SSH_ASKPASS".into(), exe.to_string_lossy().into_owned()),
                ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
                ("DISPLAY".into(), "oo:0".into()),
                (ASKPASS.into(), name.clone()),
            ];
        }
        (argv, env)
    }

    /// One agent call. `stream` follows the request line on the same stdin.
    fn call(
        &self,
        action: &str,
        mut request: Value,
        stream: Option<ChildStdout>,
    ) -> Result<Value, Failure> {
        request["protocol"] = json!(PROTOCOL);
        request["action"] = json!(action);
        let mut child = self
            .command()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| fail("SSH_CLIENT_UNAVAILABLE: install the OpenSSH client"))?;
        let mut stdin = child.stdin.take().ok_or_else(|| fail("SSH_STDIN"))?;
        let mut body = format!("{}\n", AGENT.len()).into_bytes();
        body.extend_from_slice(AGENT.as_bytes());
        body.extend_from_slice(request.to_string().as_bytes());
        body.push(b'\n');
        let writer = std::thread::spawn(move || -> bool {
            let mut sent = stdin.write_all(&body).is_ok();
            if let Some(mut source) = stream {
                sent = sent && std::io::copy(&mut source, &mut stdin).is_ok();
            }
            drop(stdin);
            sent
        });
        let mut stderr = child.stderr.take().ok_or_else(|| fail("SSH_STDERR"))?;
        let errors = std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = (&mut stderr).take(65_536).read_to_end(&mut text);
            let _ = std::io::copy(&mut stderr, &mut std::io::sink());
            String::from_utf8_lossy(&text).into_owned()
        });
        let mut out = Vec::new();
        if let Some(mut stdout) = child.stdout.take() {
            let _ = (&mut stdout).take(4 * 1_048_576).read_to_end(&mut out);
        }
        let status = child.wait().map_err(|_| fail("SSH_WAIT"))?;
        let sent = writer.join().unwrap_or(false);
        let errors = errors.join().unwrap_or_default();
        if status.code() == Some(255) {
            return Err(classify(&errors));
        }
        let value: Value = serde_json::from_slice(&out)
            .ok()
            .filter(|v: &Value| v["protocol"] == PROTOCOL)
            .ok_or_else(|| {
                fail(
                    if errors.contains("python3: not found") || status.code() == Some(127) {
                        "SSH_REMOTE_PYTHON_REQUIRED: install python3 on the target host"
                    } else {
                        "SSH_REMOTE_PROTOCOL"
                    },
                )
            })?;
        if !sent && value["ok"] == true {
            return Err(fail("SSH_STREAM_INCOMPLETE"));
        }
        Ok(value)
    }
}

fn classify(stderr: &str) -> Failure {
    if stderr.contains("Host key verification failed")
        || stderr.contains("HOST IDENTIFICATION HAS CHANGED")
        || stderr.contains("No matching host key")
    {
        fail("SSH_HOST_KEY_MISMATCH: the host did not prove the pinned key")
    } else if stderr.contains("Permission denied") || stderr.contains("Too many authentication") {
        fail("SSH_AUTH_FAILED")
    } else {
        fail("SSH_CONNECT_FAILED")
    }
}

fn private_dir(path: &Path) -> Result<(), Failure> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(fail("SSH_STATE_DIRECTORY")),
    }
    let meta = fs::symlink_metadata(path).map_err(|_| fail("SSH_STATE_DIRECTORY"))?;
    if !meta.is_dir() || meta.permissions().mode() & 0o777 != 0o700 {
        return Err(fail("SSH_STATE_DIRECTORY"));
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .map_err(|_| fail("SSH_STATE_WRITE"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| fail("SSH_STATE_WRITE"))?;
    fs::rename(&tmp, path).map_err(|_| fail("SSH_STATE_WRITE"))
}

fn algorithms(key_type: &str) -> Option<&'static str> {
    Some(match key_type {
        "ssh-ed25519" => "ssh-ed25519",
        "ecdsa-sha2-nistp256" => "ecdsa-sha2-nistp256",
        "ecdsa-sha2-nistp384" => "ecdsa-sha2-nistp384",
        "ecdsa-sha2-nistp521" => "ecdsa-sha2-nistp521",
        "ssh-rsa" => "rsa-sha2-512,rsa-sha2-256",
        _ => return None,
    })
}

/// Keep only the scanned key whose fingerprint is pinned; refuse otherwise.
fn pin(ssh: &DeploySsh, state: &Path) -> Result<&'static str, Failure> {
    let scan = Command::new("ssh-keyscan")
        .args(["-T", "10", "-p", &ssh.port.to_string(), "--", &ssh.host])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| fail("SSH_CLIENT_UNAVAILABLE: install the OpenSSH client"))?;
    let text = String::from_utf8_lossy(&scan.stdout);
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.starts_with('#') && l.split_whitespace().count() == 3)
        .collect();
    if lines.is_empty() {
        return Err(fail("SSH_UNREACHABLE: no host key was presented"));
    }
    let probe = state.join("scan");
    let mut offered = Vec::new();
    for line in lines {
        let key_type = line.split_whitespace().nth(1).unwrap_or("");
        let Some(algorithms) = algorithms(key_type) else {
            continue;
        };
        write_private(&probe, format!("{line}\n").as_bytes())?;
        let print = Command::new("ssh-keygen")
            .args(["-l", "-E", "sha256", "-f"])
            .arg(&probe)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| fail("SSH_CLIENT_UNAVAILABLE: install the OpenSSH client"))?;
        let _ = fs::remove_file(&probe);
        let printed = String::from_utf8_lossy(&print.stdout);
        let Some(fingerprint) = printed.split_whitespace().nth(1) else {
            continue;
        };
        if fingerprint == ssh.host_key_sha256 {
            write_private(&state.join("known_hosts"), format!("{line}\n").as_bytes())?;
            return Ok(algorithms);
        }
        offered.push(format!("{fingerprint} ({key_type})"));
    }
    Err(Failure::local(format!(
        "SSH_HOST_KEY_MISMATCH: the pinned host_key_sha256 was not presented; host offered {}",
        offered.join(", ")
    )))
}

fn connect(ssh: &DeploySsh, state: &Path) -> Result<Remote, Failure> {
    let algorithms = pin(ssh, state)?;
    let store = oo_stack::secret::SecretStore::from_home().map_err(Failure::local)?;
    let known = state.join("known_hosts");
    let mut options: Vec<OsString> = Vec::new();
    let mut opt = |value: String| {
        options.push("-o".into());
        options.push(value.into());
    };
    opt(format!("UserKnownHostsFile={}", known.display()));
    for value in [
        "StrictHostKeyChecking=yes",
        "GlobalKnownHostsFile=/dev/null",
        "UpdateHostKeys=no",
        "CheckHostIP=no",
        "ConnectTimeout=15",
        "ServerAliveInterval=15",
        "ServerAliveCountMax=4",
        "LogLevel=ERROR",
        "ForwardAgent=no",
        "ForwardX11=no",
        "ClearAllForwardings=yes",
        "IdentityAgent=none",
        "IdentitiesOnly=yes",
        "RequestTTY=no",
    ] {
        opt(value.into());
    }
    opt(format!("HostKeyAlgorithms={algorithms}"));
    let password = if let Some(key) = &ssh.key {
        for value in [
            "BatchMode=yes",
            "PubkeyAuthentication=yes",
            "PasswordAuthentication=no",
            "KbdInteractiveAuthentication=no",
        ] {
            opt(value.into());
        }
        let path = store.path(&key.secret_ref).map_err(Failure::local)?;
        opt(format!("IdentityFile={}", path.display()));
        None
    } else {
        let reference = ssh
            .password
            .as_ref()
            .ok_or_else(|| fail("STACK_DEPLOY_SSH"))?;
        store.read(&reference.secret_ref).map_err(Failure::local)?;
        for value in [
            "PubkeyAuthentication=no",
            "PreferredAuthentications=password,keyboard-interactive",
            "NumberOfPasswordPrompts=1",
        ] {
            opt(value.into());
        }
        Some(reference.secret_ref.clone())
    };
    let mut all: Vec<OsString> = vec!["-F".into(), "/dev/null".into(), "-T".into()];
    all.extend(options);
    all.extend(["-p".into(), ssh.port.to_string().into()]);
    all.extend(["-l".into(), ssh.user.clone().into()]);
    Ok(Remote {
        options: all,
        host: ssh.host.clone(),
        password,
    })
}

fn local_image(reference: &str, arch: &str) -> Result<Value, Failure> {
    let out = Command::new("docker")
        .args([
            "image",
            "inspect",
            "--platform",
            &format!("linux/{arch}"),
            "--format",
            "{{json .Id}} {{json .Os}} {{json .Architecture}} {{json .RootFS.Layers}}",
            reference,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| fail("SSH_LOCAL_DOCKER"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let parts: Vec<&str> = text.trim().splitn(4, ' ').collect();
    if !out.status.success() || parts.len() != 4 {
        return Err(fail("SSH_LOCAL_IMAGE_UNAVAILABLE"));
    }
    let parse = |s: &str| serde_json::from_str::<Value>(s).map_err(|_| fail("SSH_LOCAL_IMAGE"));
    let info = json!({"id":parse(parts[0])?,"os":parse(parts[1])?,"arch":parse(parts[2])?,
        "layers":parse(parts[3])?});
    if info["os"] != "linux" || info["arch"] != arch {
        return Err(fail("SSH_LOCAL_IMAGE_PLATFORM"));
    }
    Ok(info)
}

fn same_content(remote: &Value, local: &Value) -> bool {
    remote.is_object()
        && ["os", "arch", "layers"]
            .iter()
            .all(|k| remote[*k] == local[*k])
}

/// Local runtime image for the remote architecture; always a content digest.
fn runtime_reference(arch: &str) -> Result<String, Failure> {
    let upper = arch.to_ascii_uppercase();
    let companion = |name: &str| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| fs::read_to_string(exe.with_file_name(name)).ok())
            .map(|s| s.trim().to_owned())
    };
    let native = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    };
    let image = std::env::var(format!("OO_LITE_RUNTIME_IMAGE_{upper}"))
        .ok()
        .or_else(|| companion(&format!("oo-lite-image-{arch}")))
        .or_else(|| {
            (arch == native)
                .then(|| {
                    std::env::var("OO_LITE_RUNTIME_IMAGE")
                        .ok()
                        .or_else(|| companion("oo-lite-image"))
                })
                .flatten()
        })
        .unwrap_or_default();
    let digest = image.rsplit_once('@').map_or(image.as_str(), |(_, v)| v);
    if !digest
        .strip_prefix("sha256:")
        .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(Failure::local(format!(
            "SSH_RUNTIME_IMAGE_DIGEST_REQUIRED: set OO_LITE_RUNTIME_IMAGE_{upper} to the linux/{arch} runtime content digest"
        )));
    }
    Ok(image)
}

fn postgres_reference() -> Result<&'static str, Failure> {
    COMPOSE
        .lines()
        .filter_map(|l| l.trim().strip_prefix("image: "))
        .find(|r| r.contains("postgres@sha256:"))
        .ok_or_else(|| fail("SSH_COMPOSE_TEMPLATE"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn checked(value: Value) -> Result<Value, Failure> {
    if value["ok"] == true {
        Ok(value)
    } else {
        Err(Failure::local(
            value["error"]
                .as_str()
                .unwrap_or("SSH_REMOTE_FAILED")
                .to_owned(),
        ))
    }
}

/// Ensure one image on the remote; returns the remote reference compose uses.
fn ensure_image(
    remote: &Remote,
    role: &str,
    local_ref: &str,
    pinned: Option<&str>,
    arch: &str,
    transfers: &mut Vec<Value>,
) -> Result<String, Failure> {
    let local = local_image(local_ref, arch);
    let key = local
        .as_ref()
        .ok()
        .and_then(|v| v["id"].as_str())
        .map(|id| {
            id.trim_start_matches("sha256:")
                .chars()
                .take(12)
                .collect::<String>()
        });
    let key = match (&key, pinned) {
        (Some(k), _) => k.clone(),
        (None, Some(p)) => p
            .rsplit_once("sha256:")
            .map_or("", |(_, v)| &v[..12])
            .to_owned(),
        _ => return Err(local.err().unwrap_or_else(|| fail("SSH_LOCAL_IMAGE"))),
    };
    let tag = format!("open-ontology/lite-{role}:{key}-{arch}");
    let refs: Vec<&str> = std::iter::once(tag.as_str()).chain(pinned).collect();
    let found = checked(remote.call("images", json!({"refs":refs}), None)?)?;
    if let Some(p) = pinned
        && found["images"][p].is_object()
    {
        transfers.push(json!({"role":role,"image":p,"transfer":"present"}));
        return Ok(p.to_owned());
    }
    if let Ok(local) = &local
        && same_content(&found["images"][&tag], local)
    {
        transfers.push(json!({"role":role,"image":tag,"transfer":"present"}));
        return Ok(found["images"][&tag]["id"]
            .as_str()
            .unwrap_or_default()
            .to_owned());
    }
    if let Some(p) = pinned {
        let pulled = checked(remote.call("pull", json!({"ref":p}), None)?)?;
        if pulled["error"].is_null() {
            transfers.push(json!({"role":role,"image":p,"transfer":"pulled_by_digest"}));
            return Ok(p.to_owned());
        }
    }
    let local = local?;
    let mut save = Command::new("docker")
        .args(["save", "--platform", &format!("linux/{arch}"), local_ref])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| fail("SSH_LOCAL_DOCKER"))?;
    let loaded = remote.call(
        "load",
        json!({"tag":tag,"expect":{"os":local["os"],"arch":local["arch"],"layers":local["layers"]}}),
        save.stdout.take(),
    );
    let saved = save.wait().map(|s| s.success()).unwrap_or(false);
    let loaded = checked(loaded?)?;
    if !saved {
        return Err(fail("SSH_LOCAL_IMAGE_SAVE"));
    }
    transfers.push(
        json!({"role":role,"image":tag,"transfer":"docker_save_load",
        "verified":"os, architecture and RootFS layer digests"}),
    );
    Ok(loaded["image"].as_str().unwrap_or_default().to_owned())
}

pub(super) fn run(action: &str, path: &Path, stack: &Stack) -> Result<Invocation, Failure> {
    let ssh = stack
        .deploy
        .ssh
        .as_ref()
        .ok_or_else(|| fail("SSH_TARGET_REQUIRED: add deploy.ssh to stack.yaml"))?;
    if stack.deploy.profile != "lite" {
        return Err(fail("SSH_LITE_ONLY"));
    }
    let config = fs::canonicalize(path).map_err(|_| fail("ADMIN_CONFIG_IO"))?;
    let dot = config
        .parent()
        .ok_or_else(|| fail("ADMIN_CONFIG_IO"))?
        .join(".oo");
    let state = dot.join("ssh");
    for dir in [&dot, &state] {
        private_dir(dir)?;
    }
    let remote = connect(ssh, &state)?;
    // Credentials are created and inspected here; without an Ed25519-capable
    // host OpenSSL, use the local pinned runtime image (requires local Docker).
    let _openssl = (action == "up" || state.join("pki").exists()).then(|| {
        super::lite::OpensslHelper::start(&state.join("pki"), &super::lite::pinned_image())
    });
    let mut summary = json!({"host":ssh.host,"port":ssh.port,"user":ssh.user,
        "host_key_sha256":ssh.host_key_sha256,"host_key":"pinned"});
    match action {
        "up" => up(&remote, stack, &state, summary),
        "status" => {
            let reply = checked(remote.call("status", json!({}), None)?)?;
            summary["timer"] = reply["timer"].clone();
            summary["installed"] = reply["installed"].clone();
            report(reply, summary, &state)
        }
        "down" => {
            let reply = checked(remote.call("down", json!({}), None)?)?;
            let clean = reply["remaining"].as_object().is_some_and(|m| m.is_empty());
            if clean {
                fs::remove_dir_all(&state).map_err(|_| fail("SSH_STATE_REMOVE"))?;
            }
            summary["removed"] = reply["removed"].clone();
            summary["remaining"] = reply["remaining"].clone();
            let out = json!({"status": if clean {"removed"} else {"incomplete"},
                "target":"ssh","remote":summary,"healthy":false,
                "local_state":if clean {"removed"} else {"retained"}});
            Ok(Invocation::AdminReport(out.to_string(), u8::from(!clean)))
        }
        _ => Err(fail("ADMIN_ARGUMENT")),
    }
}

/// D16 for `--target ssh`: each phase runs the pinned runtime image on the
/// remote host (same container as local lite) through the pinned connection;
/// owner review and the review compiler's caller stay on this host.
pub(super) fn build(
    path: &Path,
    stack: &Stack,
    resume: bool,
    json_output: bool,
    review_file: Option<&Path>,
    cycle: Option<&str>,
) -> Result<Invocation, Failure> {
    use std::io::IsTerminal;
    let ssh = stack
        .deploy
        .ssh
        .as_ref()
        .ok_or_else(|| fail("SSH_TARGET_REQUIRED: add deploy.ssh to stack.yaml"))?;
    let config = fs::canonicalize(path).map_err(|_| fail("ADMIN_CONFIG_IO"))?;
    let dot = config
        .parent()
        .ok_or_else(|| fail("ADMIN_CONFIG_IO"))?
        .join(".oo");
    let state = dot.join("ssh");
    if !state.join("pki").exists() {
        return Err(fail(
            "BUILD_LITE_NOT_PROVISIONED: run oo up --target ssh first",
        ));
    }
    let remote = connect(ssh, &state)?;
    let mut fresh = !resume;
    let mut run_phase = true;
    for _ in 0..3 {
        let reply = checked(remote.call(
            "build",
            json!({"fresh":fresh,"run":run_phase,"cycle":cycle}),
            None,
        )?)?;
        fresh = false;
        run_phase = true;
        let chain = &reply["chain"];
        if chain["status"] == "complete" {
            // Reconcile: arms the scheduled cycle and starts the Gateway.
            if let Invocation::AdminReport(text, _) = self::run("up", path, stack)? {
                let up: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if !matches!(up["status"].as_str(), Some("retained_ready" | "healthy")) {
                    return Err(fail("BUILD_LITE_GATEWAY_SERVICE"));
                }
            }
            let mut out = super::build::report(chain, json_output);
            if !json_output && let Invocation::AdminReport(text, _) = &mut out {
                text.push_str("\n\n");
                text.push_str(
                    reply["prompt"]
                        .as_str()
                        .ok_or_else(|| fail("BUILD_PROMPT_RECEIPT"))?,
                );
            }
            return Ok(out);
        }
        let Some(input) = reply.get("review_input").filter(|_| {
            cycle.is_none()
                && chain["stages"][2]["status"] == "complete"
                && chain["stages"][3]["status"] == "blocked"
        }) else {
            return Ok(super::build::report(chain, json_output));
        };
        if review_file.is_none()
            && !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal())
        {
            let copy = dot.join("review-input.json");
            if !copy.exists() {
                super::review::save_new(&copy, input)?;
            } else if super::review::read(&copy)? != *input {
                return Err(fail("BUILD_REVIEW_INPUT_CHANGED"));
            }
            return Ok(super::build::report(chain, json_output));
        }
        let image = super::build_retained::text(&reply, "image")?;
        let lane = super::lite::lane(Path::new("/nonexistent"));
        let compiler = super::build_lite::compiler_argv(&lane, None, image)
            .iter()
            .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" ");
        let (argv, env) = remote.plain(&compiler);
        let reviewed = super::build_lite::review_on_host(&dot, input, review_file, argv, env)?;
        checked(remote.call("handoff", json!({"reviewed":reviewed}), None)?)?;
    }
    Err(fail("BUILD_LITE_NO_PROGRESS"))
}

fn report(reply: Value, summary: Value, state: &Path) -> Result<Invocation, Failure> {
    let mut out = reply["lifecycle"].clone();
    if !out.is_object() {
        out = json!({"status":"failed","error":"SSH_REMOTE_LIFECYCLE","healthy":false});
    }
    out["target"] = json!("ssh");
    out["remote"] = summary;
    let pki = state.join("pki");
    if pki.exists() {
        out["bootstrap_credentials"] = match oo_stack::credentials::status(&pki) {
            Ok(status) => json!(status),
            Err(_) => json!({"status":"invalid","nearest_credential_expiry":null}),
        };
    }
    // Same contract as local lite: provisioned for `oo build` (retained_ready)
    // or healthy exits 0; anything else exits 1.
    let code = if out["status"] == "not_started" {
        0
    } else {
        u8::from(
            !(out["healthy"] == true || out["status"] == "retained_ready")
                || reply["lifecycle_exit"] != 0,
        )
    };
    Ok(Invocation::AdminReport(out.to_string(), code))
}

fn up(
    remote: &Remote,
    stack: &Stack,
    state: &Path,
    mut summary: Value,
) -> Result<Invocation, Failure> {
    let store = oo_stack::secret::SecretStore::from_home().map_err(Failure::local)?;
    let mut refs = vec![stack.model.key.secret_ref.clone()];
    if let ModelEndpoint::Secret(reference) = &stack.model.base_url {
        refs.push(reference.secret_ref.clone());
    }
    for source in &stack.sources {
        refs.push(source.password.secret_ref.clone());
        if let Some(ca) = &source.tls_ca {
            refs.push(ca.secret_ref.clone());
        }
    }
    refs.sort();
    refs.dedup();
    let mut secrets = serde_json::Map::new();
    for name in &refs {
        let value = store.read(name).map_err(Failure::local)?;
        secrets.insert(name.clone(), json!(hex(value.as_bytes())));
    }
    let probe = checked(remote.call("probe", json!({}), None)?)?;
    for key in ["arch", "docker", "compose", "systemd", "scope", "python"] {
        summary[key] = probe[key].clone();
    }
    if probe["ready"] != true {
        let out = json!({"status":"failed","target":"ssh","error":"SSH_REMOTE_PREREQUISITES",
            "missing":probe["missing"],"guidance":probe["guidance"],"remote":summary,"healthy":false});
        return Ok(Invocation::AdminReport(out.to_string(), 1));
    }
    let arch = probe["arch"].as_str().unwrap_or_default().to_owned();
    let mut transfers = Vec::new();
    let runtime_ref = runtime_reference(&arch)?;
    let runtime = ensure_image(remote, "runtime", &runtime_ref, None, &arch, &mut transfers)?;
    let pinned = postgres_reference()?;
    let postgres = ensure_image(
        remote,
        "postgres",
        pinned,
        Some(pinned),
        &arch,
        &mut transfers,
    )?;
    summary["images"] = json!(transfers);
    // The pinned digest stays authoritative; an offline copy is referenced by
    // the ID that was verified against the local pinned image.
    let compose = COMPOSE.replace(&format!("image: {pinned}"), &format!("image: {postgres}"));
    let pki = state.join("pki");
    oo_stack::credentials::bootstrap(&pki).map_err(Failure::local)?;
    oo_stack::credentials::workload_identity(&pki, "consumer").map_err(Failure::local)?;
    let bootstrap = oo_stack::secret::SecretStore::open(&pki).map_err(Failure::local)?;
    // The owner's CLI reaches the remote Gateway with its own local copies.
    let consumer = state.join("consumer");
    private_dir(&consumer)?;
    for (name, parts) in [
        ("ca.crt", &["lite-ca-cert"][..]),
        (
            "client.pem",
            &["lite-consumer-cert", "lite-consumer-key"][..],
        ),
    ] {
        let mut bytes = Vec::new();
        for part in parts {
            bytes.extend(bootstrap.read(part).map_err(Failure::local)?.into_bytes());
        }
        let path = consumer.join(name);
        if fs::read(&path).ok().as_deref() != Some(&bytes[..]) {
            write_private(&path, &bytes)?;
        }
    }
    let text = |path: PathBuf| path.to_string_lossy().into_owned();
    let mut files = serde_json::Map::new();
    for entry in fs::read_dir(&pki).map_err(|_| fail("SSH_STATE_PKI"))? {
        let name = entry
            .map_err(|_| fail("SSH_STATE_PKI"))?
            .file_name()
            .into_string()
            .map_err(|_| fail("SSH_STATE_PKI"))?;
        let value = bootstrap.read(&name).map_err(Failure::local)?;
        files.insert(name, json!(hex(value.as_bytes())));
    }
    let mut remote_stack = stack.clone();
    remote_stack.deploy.target = "local".into();
    remote_stack.deploy.ssh = None;
    let interval = stack
        .sources
        .iter()
        .filter_map(|s| interval_seconds(&s.sync.interval))
        .min()
        .unwrap_or(8 * 3600);
    let reply = checked(remote.call(
        "install",
        json!({"stack":remote_stack,"image":runtime,"pki":files,"secrets":secrets,
            "templates":{"compose.yaml":compose,"postgres.sh":POSTGRES_SH,"scheduler.py":SCHEDULER},
            "retained":true,"jobs":super::lite::jobs(),
            "prompt":{"ca_file":text(consumer.join("ca.crt")),"client_pem":text(consumer.join("client.pem"))},
            "lifecycle":LIFECYCLE,"agent":AGENT,"scope":probe["scope"],"python3":probe["python3"],
            "interval_seconds":interval}),
        None,
    )?)?;
    summary["timer"] = reply["timer"].clone();
    let transferred = transfers.iter().any(|t| t["transfer"] != "present");
    let mut changed = reply["changed"].as_array().cloned().unwrap_or_default();
    if transferred {
        changed.push(json!("images"));
    }
    summary["changed"] = json!(changed);
    summary["noop"] = json!(changed.is_empty());
    report(reply, summary, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_and_agent_never_carry_values_in_argv() {
        assert_eq!(LOADER.matches('\'').count(), 2);
        assert!(AGENT.contains(PROTOCOL));
        assert_eq!(
            postgres_reference()
                .ok()
                .unwrap()
                .split_once("@sha256:")
                .unwrap()
                .1
                .len(),
            64
        );
    }

    #[test]
    fn host_key_algorithms_are_restricted_to_the_pinned_type() {
        assert_eq!(algorithms("ssh-ed25519"), Some("ssh-ed25519"));
        assert_eq!(algorithms("ssh-rsa"), Some("rsa-sha2-512,rsa-sha2-256"));
        assert_eq!(algorithms("ssh-dss"), None);
    }

    #[test]
    fn ssh_failures_are_classified_without_relaying_output() {
        let mismatch = classify("@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! secret");
        assert!(!format!("{}", String::from_utf8_lossy(&mismatch.json_bytes())).contains("secret"));
        assert!(
            String::from_utf8_lossy(&classify("Permission denied (publickey).").json_bytes())
                .contains("SSH_AUTH_FAILED")
        );
    }

    #[test]
    fn content_comparison_ignores_store_specific_ids() {
        let a = json!({"id":"sha256:1","os":"linux","arch":"arm64","layers":["sha256:x"]});
        let b = json!({"id":"sha256:2","os":"linux","arch":"arm64","layers":["sha256:x"]});
        assert!(same_content(&a, &b));
        assert!(!same_content(&Value::Null, &b));
        assert_eq!(hex(b"\x00\xff"), "00ff");
    }
}
