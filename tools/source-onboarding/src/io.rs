use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn require(ok: bool, reason: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(reason.into()) }
}
pub fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().ok_or_else(|| format!("field:{key}").into())
}
pub fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    v[key]
        .as_array()
        .ok_or_else(|| format!("array:{key}").into())
}
pub fn load(p: impl AsRef<Path>) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn current_uid() -> Result<u32> {
    // This process never changes identity; do not fork `id` for every pinned file.
    static UID: OnceLock<u32> = OnceLock::new();
    if let Some(uid) = UID.get() {
        return Ok(*uid);
    }
    let output = Command::new("id").arg("-u").output()?;
    require(output.status.success(), "process_identity")?;
    let uid = String::from_utf8(output.stdout)?.trim().parse::<u32>()?;
    let _ = UID.set(uid);
    Ok(uid)
}
pub fn private(p: impl AsRef<Path>) -> Result<PathBuf> {
    let p = p.as_ref();
    let m = fs::symlink_metadata(p)?;
    let uid = current_uid()?;
    require(
        p.is_absolute()
            && m.is_file()
            && !m.file_type().is_symlink()
            && m.permissions().mode() & 0o077 == 0
            && m.uid() == uid,
        "private_file",
    )?;
    Ok(p.to_path_buf())
}
pub fn dir(p: impl AsRef<Path>) -> Result<()> {
    let p = p.as_ref();
    if p.exists() {
        let m = fs::symlink_metadata(p)?;
        require(
            m.is_dir() && !m.file_type().is_symlink() && m.permissions().mode() & 0o077 == 0,
            "private_directory",
        )?;
    } else {
        fs::create_dir_all(p)?;
        fs::set_permissions(p, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn bytes(v: &Value) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(v)?)
}
pub fn sha(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
pub fn digest(p: impl AsRef<Path>) -> Result<String> {
    Ok(sha(&fs::read(p)?))
}
pub fn put(p: impl AsRef<Path>, b: &[u8]) -> Result<()> {
    let p = p.as_ref();
    if p.exists() {
        require(
            !fs::symlink_metadata(p)?.file_type().is_symlink() && fs::read(p)? == b,
            "immutable_output_drift",
        )?;
        return Ok(());
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(p)?;
    f.write_all(b)?;
    f.sync_all()?;
    Ok(())
}
pub fn save(p: impl AsRef<Path>, v: &Value) -> Result<()> {
    let mut b = bytes(v)?;
    b.push(b'\n');
    put(p, &b)
}
pub fn copy(a: impl AsRef<Path>, b: impl AsRef<Path>) -> Result<()> {
    put(b, &fs::read(a)?)
}
pub fn ident(s: &str) -> Result<&str> {
    require(
        !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
        "identifier",
    )?;
    Ok(s)
}
pub fn binary(p: &str) -> Result<()> {
    require(
        Path::new(p)
            .parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            == Some(&digest(p)?),
        "binary_pin",
    )
}
pub fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}

/// A child command exited non-zero. Its stderr is kept so a caller can tell one specific
/// refusal from every other failure; the message is unchanged (`command_failed:<label>`).
#[derive(Debug)]
pub struct CommandFailed {
    pub label: String,
    pub stderr: Vec<u8>,
}
impl std::fmt::Display for CommandFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "command_failed:{}", self.label)
    }
}
impl std::error::Error for CommandFailed {}

pub const DEFAULT_CHILD_DEADLINE_MS: u64 = 600_000;
const CHILD_DEADLINE_MIN_MS: u64 = 1_000;
const CHILD_DEADLINE_MAX_MS: u64 = 43_200_000;
/// Labels that may carry a per-label deadline (`onboarding.child_deadlines_ms`). These are
/// the worker subcommands the capture and serve loops run; any other key is refused.
pub const OVERRIDABLE_LABELS: [&str; 5] = [
    "native-put",
    "native-state",
    "native-fire",
    "native-step",
    "native-artifacts",
];

/// Wall-clock limit per child command. `attempt()` kills the child when its limit passes.
#[derive(Clone, Debug, PartialEq)]
pub struct Deadlines {
    pub default: Duration,
    pub per_label: BTreeMap<String, Duration>,
}
impl Default for Deadlines {
    fn default() -> Self {
        Self {
            default: Duration::from_millis(DEFAULT_CHILD_DEADLINE_MS),
            per_label: BTreeMap::new(),
        }
    }
}
impl Deadlines {
    pub fn for_label(&self, label: &str) -> Duration {
        self.per_label.get(label).copied().unwrap_or(self.default)
    }
    /// `onboarding.child_deadline_ms` (default 600000) and `onboarding.child_deadlines_ms`
    /// (`{label: ms}`, labels from `OVERRIDABLE_LABELS`). Each value is 1000..=43200000.
    pub fn from_operator(o: &Value) -> Result<Self> {
        let default = match o.get("child_deadline_ms").filter(|v| !v.is_null()) {
            Some(v) => child_deadline(v)?,
            None => Duration::from_millis(DEFAULT_CHILD_DEADLINE_MS),
        };
        let mut per_label = BTreeMap::new();
        if let Some(map) = o.get("child_deadlines_ms").filter(|v| !v.is_null()) {
            for (label, v) in map.as_object().ok_or("child_deadlines_ms")? {
                require(
                    OVERRIDABLE_LABELS.contains(&label.as_str()),
                    "child_deadline_label",
                )?;
                per_label.insert(label.clone(), child_deadline(v)?);
            }
        }
        Ok(Self { default, per_label })
    }
}
fn child_deadline(v: &Value) -> Result<Duration> {
    let ms = v.as_u64().ok_or("child_deadline_ms")?;
    require(
        (CHILD_DEADLINE_MIN_MS..=CHILD_DEADLINE_MAX_MS).contains(&ms),
        "child_deadline_ms",
    )?;
    Ok(Duration::from_millis(ms))
}

pub struct Calls {
    pub out: PathBuf,
    pub deadlines: Deadlines,
    n: usize,
}
impl Calls {
    pub fn new(out: &Path) -> Result<Self> {
        dir(out)?;
        let n = fs::read_dir(out)?.count();
        Ok(Self {
            out: out.into(),
            deadlines: Deadlines::default(),
            n,
        })
    }
    /// Like `new`, with the deadlines of the operator's `onboarding` block. Refuses an invalid
    /// deadline before the output directory is created.
    pub fn with_operator(out: &Path, operator: &Value) -> Result<Self> {
        let deadlines = Deadlines::from_operator(operator)?;
        let mut calls = Self::new(out)?;
        calls.deadlines = deadlines;
        Ok(calls)
    }
    pub fn run(&mut self, label: &str, args: &[String], input: Option<&[u8]>) -> Result<Vec<u8>> {
        for attempt in 0..3 {
            let (status, stdout, stderr) = self.attempt(label, args, input)?;
            if status.success() {
                return Ok(stdout);
            }
            if attempt == 2 || !retry_native_fire(label, args, input, status.code(), &stderr) {
                return Err(Box::new(CommandFailed {
                    label: label.into(),
                    stderr,
                }));
            }
            save(
                self.out.join(format!("{:04}-transport-retry.json", self.n)),
                &json!({"command":label,"attempt":attempt+1,"maximum_attempts":3,
                    "stderr_sha256":sha(&stderr),"reason":"docker_transport_disconnected",
                    "scheduler_transaction_reentered":label.ends_with("native-fire"),
                    "read_only_repeated":label.starts_with("incremental-authority-")}),
            )?;
            std::thread::sleep(Duration::from_secs(1 << attempt));
        }
        unreachable!()
    }
    fn attempt(
        &mut self,
        label: &str,
        args: &[String],
        input: Option<&[u8]>,
    ) -> Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>)> {
        self.n += 1;
        let prefix = self.out.join(format!("{:04}-{label}", self.n));
        save(
            prefix.with_extension("started.json"),
            &json!({"argv":args,"input_sha256":input.map(sha)}),
        )?;
        let mut child = Command::new(&args[0])
            .args(&args[1..])
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::from(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(prefix.with_extension("stdout"))?,
            ))
            .stderr(Stdio::from(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(prefix.with_extension("stderr"))?,
            ))
            .spawn()?;
        if let Some(data) = input {
            child.stdin.take().ok_or("child_stdin")?.write_all(data)?;
        }
        let deadline = self.deadlines.for_label(label);
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if start.elapsed() >= deadline {
                child.kill()?;
                let status = child.wait()?;
                save(
                    prefix.with_extension("exit.json"),
                    &json!({"exit":status.code(),"success":false,"elapsed_ms":start.elapsed().as_millis(),
                        "child_deadline_ms":deadline.as_millis(),"deadline_exceeded":true,"killed":true,"signal":status.signal()}),
                )?;
                return Err("child_deadline".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        save(
            prefix.with_extension("exit.json"),
            &json!({"exit":status.code(),"success":status.success(),"elapsed_ms":start.elapsed().as_millis()}),
        )?;
        Ok((
            status,
            fs::read(prefix.with_extension("stdout"))?,
            fs::read(prefix.with_extension("stderr"))?,
        ))
    }
    pub fn docker(
        &mut self,
        label: &str,
        container: &str,
        args: &[String],
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let mut argv = strings(&["docker", "exec"]);
        if input.is_some() {
            argv.push("-i".into())
        };
        argv.push(container.into());
        argv.extend_from_slice(args);
        self.run(label, &argv, input)
    }
}

// Only scheduling or a verified read-only export can replay an unobserved exec.
fn retry_native_fire(label: &str, args: &[String], input: Option<&[u8]>, exit: Option<i32>, stderr: &[u8]) -> bool {
    let error = String::from_utf8_lossy(stderr);
    let error = error.trim();
    let schedule = matches!(label, "native-fire" | "incremental-native-fire")
        && args.iter().any(|arg| arg == "native-fire");
    let export = matches!(label, "incremental-authority-before" | "incremental-authority-export" | "incremental-authority-progress")
        && args.last().is_some_and(|arg| arg == "native-c5-state")
        && input.and_then(|b| serde_json::from_slice::<Value>(b).ok()).is_some_and(|v|
            v["protocol"] == "oo.pipeline.native-c5-state/v1" && v["operation"] == "export"
                && v.get("authority_file").is_none());
    (schedule || export)
        && exit == Some(1)
        && args.first().is_some_and(|arg| {
            Path::new(arg)
                .file_name()
                .is_some_and(|name| name == "docker")
        })
        && args.get(1).is_some_and(|arg| arg == "exec")
        && (error == "unexpected EOF"
            || error.starts_with("Cannot connect to the Docker daemon at unix://")
                && error.ends_with("Is the docker daemon running?")
            || error.starts_with("failed to connect to the docker API at unix://")
                && error.ends_with(": connect: no such file or directory"))
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    #[test]
    fn cached_identity_preserves_private_file_guards() {
        let root = std::env::temp_dir().join(format!("oo-private-identity-{}", std::process::id()));
        dir(&root).unwrap();
        let file = root.join("private.json");
        put(&file, b"{}").unwrap();
        assert_eq!(current_uid().unwrap(), fs::metadata(&file).unwrap().uid());
        for _ in 0..2000 {
            assert_eq!(private(&file).unwrap(), file);
        }
        let link = root.join("link.json");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(private(&link).is_err());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(private(&file).is_err());
        assert!(private(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_fire_reconnects_once_but_permanent_failure_is_not_replayed() {
        let root =
            std::env::temp_dir().join(format!("oo-native-fire-retry-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let docker = root.join("docker");
        fs::write(&docker,format!("#!/bin/sh\nif [ ! -e '{}' ]; then touch '{}'; echo 'unexpected EOF' >&2; exit 1; fi\necho recovered\n",root.join("once").display(),root.join("once").display())).unwrap();
        fs::set_permissions(&docker, fs::Permissions::from_mode(0o700)).unwrap();
        let args = strings(&[
            docker.to_str().unwrap(),
            "exec",
            "fixture",
            "worker",
            "native-fire",
        ]);
        let mut calls = Calls::new(&root.join("transient")).unwrap();
        assert_eq!(
            calls.run("native-fire", &args, None).unwrap(),
            b"recovered\n"
        );
        assert_eq!(calls.n, 2);
        assert!(root.join("transient/0001-transport-retry.json").exists());
        assert!(retry_native_fire("native-fire",&args,None,Some(1),b"Cannot connect to the Docker daemon at unix:///private.sock. Is the docker daemon running?"));
        assert!(retry_native_fire("native-fire", &args, None, Some(1), b"failed to connect to the docker API at unix:///private.sock; check if the path is correct and if the daemon is running: dial unix /private.sock: connect: no such file or directory"));
        assert!(!retry_native_fire(
            "native-step",
            &args,
            None,
            Some(1),
            b"unexpected EOF"
        ));
        assert!(!retry_native_fire(
            "native-fire",
            &args,
            None,
            Some(1),
            b"error=sql unexpected EOF"
        ));
        fs::write(
            &docker,
            "#!/bin/sh\necho 'Error response from daemon: unauthorized' >&2\nexit 1\n",
        )
        .unwrap();
        let mut calls = Calls::new(&root.join("permanent")).unwrap();
        assert!(calls.run("native-fire", &args, None).is_err());
        assert_eq!(calls.n, 1);
        fs::write(&docker, "#!/bin/sh\necho 'unexpected EOF' >&2\nexit 1\n").unwrap();
        let mut calls = Calls::new(&root.join("exhausted")).unwrap();
        assert!(calls.run("native-fire", &args, None).is_err());
        assert_eq!(calls.n, 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_readonly_authority_export_retries_without_replaying_capture() {
        let args=strings(&["docker","exec","-i","fixture","pipeline","native-c5-state"]);
        let mut request=json!({"protocol":"oo.pipeline.native-c5-state/v1","operation":"export"});
        assert!(retry_native_fire("incremental-authority-export",&args,Some(&bytes(&request).unwrap()),Some(1),b"unexpected EOF"));
        request["operation"]=json!("hydrate_multipart");
        assert!(!retry_native_fire("incremental-authority-export",&args,Some(&bytes(&request).unwrap()),Some(1),b"unexpected EOF"));
        request["operation"]=json!("export");request["authority_file"]=json!("/target");
        assert!(!retry_native_fire("incremental-authority-export",&args,Some(&bytes(&request).unwrap()),Some(1),b"unexpected EOF"));
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    fn scratch(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("oo-child-deadline-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        root
    }
    #[test]
    fn deadline_kills_the_child_and_records_it() {
        let root = scratch("kill");
        let mut calls = Calls::new(&root.join("calls")).unwrap();
        calls.deadlines = Deadlines {
            default: Duration::from_millis(100),
            per_label: BTreeMap::new(),
        };
        let started = Instant::now();
        let error = calls
            .run("native-step", &strings(&["/bin/sleep", "30"]), None)
            .unwrap_err();
        assert_eq!(error.to_string(), "child_deadline");
        assert!(started.elapsed() < Duration::from_secs(20));
        assert_eq!(calls.n, 1);
        let exit = load(root.join("calls/0001-native-step.exit.json")).unwrap();
        assert_eq!(exit["success"], false);
        assert_eq!(exit["exit"], Value::Null);
        assert_eq!(exit["signal"], 9);
        assert_eq!(exit["deadline_exceeded"], true);
        assert_eq!(exit["killed"], true);
        assert_eq!(exit["child_deadline_ms"], 100);
        assert!(exit["elapsed_ms"].as_u64().unwrap() >= 100);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn per_label_deadline_overrides_the_default_for_that_label_only() {
        let root = scratch("override");
        let mut calls = Calls::new(&root.join("calls")).unwrap();
        calls.deadlines = Deadlines {
            default: Duration::from_millis(100),
            per_label: BTreeMap::from([("native-step".to_string(), Duration::from_secs(10))]),
        };
        let args = strings(&["/bin/sh", "-c", "sleep 0.5; echo done"]);
        assert_eq!(calls.run("native-step", &args, None).unwrap(), b"done\n");
        let error = calls.run("native-put", &args, None).unwrap_err();
        assert_eq!(error.to_string(), "child_deadline");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn operator_deadlines_default_to_ten_minutes_for_every_label() {
        let d = Deadlines::from_operator(&json!({"producer":"/bin/true"})).unwrap();
        assert_eq!(d, Deadlines::default());
        assert_eq!(d.for_label("native-step"), Duration::from_millis(600_000));
        assert_eq!(d.for_label("native-put"), Duration::from_millis(600_000));
        let null = json!({"child_deadline_ms": null, "child_deadlines_ms": null});
        assert_eq!(Deadlines::from_operator(&null).unwrap(), Deadlines::default());
    }
    #[test]
    fn operator_native_step_override_applies_only_to_native_step() {
        let d = Deadlines::from_operator(&json!({"child_deadlines_ms":{"native-step":21600000}}))
            .unwrap();
        assert_eq!(d.for_label("native-step"), Duration::from_secs(21_600));
        assert_eq!(d.for_label("native-state"), Duration::from_secs(600));
        let global = Deadlines::from_operator(&json!({"child_deadline_ms":43200000})).unwrap();
        assert_eq!(global.for_label("native-step"), Duration::from_secs(43_200));
    }
    #[test]
    fn operator_deadlines_are_range_and_key_checked() {
        for ok in [
            json!({"child_deadline_ms": 1000}),
            json!({"child_deadline_ms": 43200000}),
            json!({"child_deadlines_ms": {"native-step": 43200000}}),
        ] {
            assert!(Deadlines::from_operator(&ok).is_ok(), "{ok}");
        }
        for bad in [
            json!({"child_deadline_ms": 999}),
            json!({"child_deadline_ms": 43200001}),
            json!({"child_deadline_ms": "600000"}),
            json!({"child_deadline_ms": -1}),
            json!({"child_deadline_ms": 1.5}),
            json!({"child_deadlines_ms": ["native-step"]}),
            json!({"child_deadlines_ms": {"native-step": 999}}),
            json!({"child_deadlines_ms": {"native-step": 43200001}}),
            json!({"child_deadlines_ms": {"native-step": "21600000"}}),
            json!({"child_deadlines_ms": {"native-step": null}}),
        ] {
            assert!(Deadlines::from_operator(&bad).is_err(), "{bad}");
        }
        for label in OVERRIDABLE_LABELS {
            let op = json!({"child_deadlines_ms": {label: 600000}});
            assert!(Deadlines::from_operator(&op).is_ok(), "{label}");
        }
        for label in ["profile-sql", "prepare", "issue-token", "native-stepp"] {
            let op = json!({"child_deadlines_ms": {label: 600000}});
            let error = Deadlines::from_operator(&op).unwrap_err();
            assert_eq!(error.to_string(), "child_deadline_label", "{label}");
        }
    }
    #[test]
    fn invalid_operator_deadline_refuses_before_the_output_directory_exists() {
        let root = scratch("refuse");
        let out = root.join("calls");
        let bad = json!({"child_deadlines_ms": {"native-step": 1}});
        assert!(Calls::with_operator(&out, &bad).is_err());
        assert!(!out.exists());
        let good = json!({"child_deadlines_ms": {"native-step": 21600000}});
        let calls = Calls::with_operator(&out, &good).unwrap_or_else(|_| panic!("valid"));
        assert_eq!(
            calls.deadlines.for_label("native-step"),
            Duration::from_secs(21_600)
        );
        fs::remove_dir_all(root).unwrap();
    }
}
