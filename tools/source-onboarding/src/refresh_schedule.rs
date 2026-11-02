//! One bounded attempt per timer wake. This owns scheduling evidence, not source
//! jobs, offsets, publication, or the refresh command's own resume artifacts.
use crate::io::{Result, bytes, load, private, require, sha, text};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CONFIG: &str = "oo.library.refresh-schedule/v1";
const STATE: &str = "oo.library.refresh-schedule-state/v1";
const INTERVAL_JOURNAL: &str = "interval-change.pending.json";
static SERIAL: AtomicU64 = AtomicU64::new(0);

pub const HELP: &str = r#"Usage:
  source-onboarding refresh-schedule run-once --config PRIVATE_SCHEDULE
  source-onboarding refresh-schedule status   --config PRIVATE_SCHEDULE
  source-onboarding refresh-schedule dry-run  --config PRIVATE_SCHEDULE
  source-onboarding refresh-schedule set-interval --config PRIVATE_SCHEDULE --seconds N
  source-onboarding refresh-schedule --help

Private JSON configuration (0600; relative paths resolve from its directory):
{
  "protocol": "oo.library.refresh-schedule/v1",
  "state_dir": "./scheduler-state",
  "start_at_ms": 1793232000000,
  "interval_seconds": 28800,
  "cycle_timeout_seconds": 21600,
  "max_attempts": 3,
  "retry_delay_seconds": 300,
  "command": {
    "argv": ["./refresh-cycle", "--out", "{cycle_dir}", "--deadline-ms", "{deadline_at_ms}"],
    "resume_argv": ["./refresh-cycle", "--out", "{cycle_dir}", "--deadline-ms", "{deadline_at_ms}", "--resume"]
  }
}

Optional "templates": {"refresh.json": "./refresh-template.json"} renders private
JSON templates into {cycle_dir}/refresh.json once per cycle. String values accept
the same placeholders; the immutable snapshot is reused and checked on resume.
For the existing seven-gate runner, use argv:
["./source-onboarding", "refresh", "--config", "{cycle_dir}/refresh.json",
 "--out", "{cycle_dir}/gates"] (append --resume in resume_argv).
Additional capture/publication JSON files can be listed in the same templates map.

Both argv arrays must contain {cycle_dir}. Also supported: {cycle_id},
{started_at_ms}, {deadline_at_ms}, {config_dir}. Arguments are NOT shell code.
The command runs with cwd=config directory, stdin=EOF, and these environment
variables: OO_REFRESH_CYCLE_DIR, OO_REFRESH_CYCLE_ID, OO_REFRESH_STARTED_AT_MS,
OO_REFRESH_DEADLINE_AT_MS, OO_REFRESH_RESUME (0/1).
Use a real, retry-safe refresh command or wrapper. It must create fresh capture,
publish and probe artifacts UNDER this cycle directory; resume must handle an
interruption even before its first artifact. Never point it at a completed run.
Reference credentials in private files/environment, not inline argv.
Exit 0 AND one JSON stdout object with success:true are required. Structured
failures in stdout/stderr are rejected; diagnostic text belongs on stderr.
Output limits: stdout 4 MiB, stderr 16 MiB. Existing refresh and
refresh-incremental receipts are supported, including failed historical attempts.
A capture-only command proves capture only, NOT publication or public freshness.

run-once takes an OS process lock and executes at most ONE attempt. Optional
start_at_ms is the initial due time/anchor (UTC Unix milliseconds). To avoid
scanning again on enable, set it to a controlled cycle's success time + 28800000.
Omit it for an immediately due first wake. Later wakes obey the fixed interval,
skipping missed slots. Changing an existing state's anchor is config drift.
Only an unfinished cycle resumes, with the SAME output directory and deadline.
Retries are bounded by max_attempts (including the first) and the original
cycle_timeout_seconds (1..43200, <= interval_seconds). Retry delay >= 1 second.
Expired/exhausted cycles wait for the next interval; no catch-up burst.
Clock rollback fails closed; wall time and monotonic time both bound execution.
Crash recovery never assumes success from old stdout. A surviving child process
group blocks reentry (including after expiry); it is NOT killed by stale PID.
Commands must remain foreground/bounded: do not daemonize or detach work.
Retain/migrate the private config AND state tree together. Config drift is
rejected for existing state; relative command/config references are relocatable.
State and per-attempt logs are private and outside any Git repository.

set-interval changes ONLY interval_seconds while idle, under the scheduler lock.
Active/pending cycles or unresolved children refuse the change without mutation.
N must be 1..31536000 and >= both cycle_timeout_seconds and retry_delay_seconds;
timeouts, retries, command/templates, start anchor and cycle evidence never change.
Next due is the first new-interval tick strictly after the change, measured from
the retained original anchor (or the unchanged future anchor). No catch-up burst.
Setting the current interval is a no-op. No state deletion or hash editing needed.
A private interval-change.pending.json journals atomic config/state replacements.
Any command finding this journal only recovers it and returns interval_recovered,
without running refresh work; rerun the requested command after recovery.
Unknown config/state drift still fails closed; do NOT edit config hashes manually.

status and dry-run are read-only except recovery of this own pending journal.
status reports last success/failure, current cycle, lock and next due/wake time.
dry-run additionally renders the proposed argv without running it.
Exit 0: inspection, success or not-due skip. Exit 2: failure, busy or invalid input.
Use launchd/systemd only to invoke run-once periodically (e.g. every 300 seconds
for retry responsiveness); the persisted interval still limits fresh cycles to
8h. There is no daemon loop and this command installs no timer.
"#;

struct Config {
    path: PathBuf,
    value: Value,
    directory: PathBuf,
    root: PathBuf,
    hash: String,
    uid: u32,
}

struct ScheduleLock(File);
impl Drop for ScheduleLock {
    fn drop(&mut self) {
        // A concurrent fork can briefly inherit a CLOEXEC descriptor. Explicit
        // unlock avoids leaving a completed invocation locked until that exec.
        let _ = self.0.unlock();
    }
}

fn fields(value: &Value, names: &[&str], optional: &[&str]) -> Result<()> {
    let object = value.as_object().ok_or("schedule_object")?;
    require(
        names.iter().all(|name| object.contains_key(*name))
            && object
                .keys()
                .all(|key| names.contains(&key.as_str()) || optional.contains(&key.as_str())),
        "schedule_fields",
    )
}

fn number(value: &Value, key: &str) -> Result<u64> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("schedule_number:{key}").into())
}

fn wall_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?)
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn private_dir(path: &Path, uid: u32, create: bool) -> Result<()> {
    if create && !path.try_exists()? {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    let metadata = fs::symlink_metadata(path)?;
    require(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == uid
            && metadata.mode() & 0o077 == 0,
        "schedule_private_directory",
    )
}

fn open_new(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?)
}

fn persist(path: &Path, value: &Value) -> Result<()> {
    let parent = path.parent().ok_or("schedule_parent")?;
    if exists(path)? {
        private(path)?;
    }
    let (pending, mut file) = loop {
        let pending = parent.join(format!(
            ".schedule-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        match open_new(&pending) {
            Ok(file) => break (pending, file),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::AlreadyExists) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
    };
    file.write_all(&bytes(value)?)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(pending, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn credential(arg: &str) -> bool {
    let lower = arg.to_ascii_lowercase();
    let option = lower
        .split('=')
        .next()
        .unwrap_or("")
        .trim_start_matches('-')
        .replace('-', "_");
    [
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
    .any(|key| option == *key || option.ends_with(&format!("_{key}")))
        || arg
            .split_once("://")
            .is_some_and(|(_, s)| s.split('/').next().unwrap_or("").contains('@'))
        || lower.starts_with("authorization:")
        || lower.starts_with("bearer ")
        || (arg.starts_with("-p") && !arg.starts_with("--"))
}

fn read_config(path: &Path) -> Result<Config> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    private(&path)?;
    config_value(&path, load(&path)?)
}

fn config_value(path: &Path, value: Value) -> Result<Config> {
    private(path)?;
    let directory = path
        .parent()
        .ok_or("schedule_config_parent")?
        .canonicalize()?;
    fields(
        &value,
        &[
            "protocol",
            "state_dir",
            "interval_seconds",
            "cycle_timeout_seconds",
            "max_attempts",
            "retry_delay_seconds",
            "command",
        ],
        &["start_at_ms", "templates"],
    )?;
    require(value["protocol"] == CONFIG, "schedule_protocol")?;
    let interval = number(&value, "interval_seconds")?;
    let timeout = number(&value, "cycle_timeout_seconds")?;
    let retry = number(&value, "retry_delay_seconds")?;
    require(
        (1..=31_536_000).contains(&interval)
            && (1..=43200).contains(&timeout)
            && timeout <= interval
            && (1..=interval).contains(&retry)
            && (1..=32).contains(&number(&value, "max_attempts")?),
        "schedule_limits",
    )?;
    if value.get("start_at_ms").is_some() {
        let start = number(&value, "start_at_ms")?;
        require(
            start > 0 && start.checked_add(interval * 1000).is_some(),
            "schedule_start_at",
        )?;
    }
    fields(&value["command"], &["argv", "resume_argv"], &[])?;
    if let Some(templates) = value.get("templates") {
        let templates = templates.as_object().ok_or("schedule_templates")?;
        require(
            !templates.is_empty() && templates.len() <= 32,
            "schedule_templates",
        )?;
        for (name, source) in templates {
            require(
                name.ends_with(".json")
                    && !["cycle.json", "templates.snapshot.json"].contains(&name.as_str())
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
                "schedule_template_name",
            )?;
            let source = source.as_str().ok_or("schedule_template_source")?;
            require(
                !source.is_empty() && !source.contains('\0'),
                "schedule_template_source",
            )?;
        }
    }
    for key in ["argv", "resume_argv"] {
        let args = value["command"][key].as_array().ok_or("schedule_argv")?;
        require(!args.is_empty() && args.len() <= 256, "schedule_argv")?;
        for arg in args {
            let arg = arg.as_str().ok_or("schedule_argv")?;
            require(
                !arg.contains('\0') && arg.len() <= 32768 && !credential(arg),
                "schedule_argv_or_inline_credentials",
            )?;
        }
        let executable = args[0].as_str().ok_or("schedule_argv")?;
        require(
            !executable.is_empty() && executable.contains('/'),
            "schedule_executable_path",
        )?;
        require(
            args.iter()
                .any(|arg| arg.as_str().unwrap().contains("{cycle_dir}")),
            "schedule_cycle_dir_required",
        )?;
    }
    let state_dir = text(&value, "state_dir")?;
    require(
        !state_dir.is_empty() && !state_dir.contains('\0'),
        "schedule_state_dir",
    )?;
    let root = crate::refresh::output_path(&directory.join(state_dir))?;
    // output_path resolves symlinks; a final-component symlink is not a state directory.
    if let Ok(metadata) = fs::symlink_metadata(directory.join(state_dir)) {
        require(!metadata.file_type().is_symlink(), "schedule_state_symlink")?;
    }
    Ok(Config {
        path: directory.join(path.file_name().ok_or("schedule_config_path")?),
        hash: sha(&bytes(&value)?),
        value,
        directory,
        root,
        uid: fs::metadata(path)?.uid(),
    })
}

fn initial(config: &Config, now: u64) -> Value {
    json!({"protocol":STATE, "config_sha256":config.hash, "sequence":0,
        "anchor_at_ms":config.value["start_at_ms"].as_u64().unwrap_or(now),
        "observed_at_ms":now, "next_due_at_ms":config.value["start_at_ms"].as_u64().unwrap_or(now), "cycle":null,
        "last_success":null, "last_failure":null})
}

fn read_state(config: &Config, now: u64) -> Result<Value> {
    if !config.root.try_exists()? {
        return Ok(initial(config, now));
    }
    private_dir(&config.root, config.uid, false)?;
    let path = config.root.join("state.json");
    if !exists(&path)? {
        // A lost state file must not turn an existing cycle into a fresh success.
        require(
            !config.root.join("cycles").try_exists()?,
            "schedule_state_missing",
        )?;
        return Ok(initial(config, now));
    }
    let state = load(private(&path)?)?;
    validate_state(config, &state)?;
    Ok(state)
}

fn validate_state(config: &Config, state: &Value) -> Result<()> {
    require(
        state["protocol"] == STATE && state["config_sha256"] == config.hash,
        "schedule_config_drift",
    )?;
    for key in ["sequence", "observed_at_ms", "next_due_at_ms"] {
        number(&state, key)?;
    }
    if state.get("anchor_at_ms").is_some() {
        number(state, "anchor_at_ms")?;
    }
    if !state["cycle"].is_null() {
        let cycle = &state["cycle"];
        let id = text(cycle, "id")?;
        require(
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit() || b == b'-'),
            "schedule_cycle_id",
        )?;
        let start = number(cycle, "started_at_ms")?;
        require(
            number(cycle, "deadline_at_ms")?.checked_sub(start)
                == Some(number(&config.value, "cycle_timeout_seconds")? * 1000),
            "schedule_budget_drift",
        )?;
        number(cycle, "due_at_ms")?;
        number(cycle, "next_attempt_at_ms")?;
        require(
            matches!(
                cycle["status"].as_str(),
                Some("running" | "retry_wait" | "succeeded" | "failed" | "expired")
            ),
            "schedule_cycle_status",
        )?;
        require(
            cycle["attempts"].as_array().is_some_and(|a| {
                a.len() <= number(&config.value, "max_attempts").unwrap_or(0) as usize
            }),
            "schedule_attempts",
        )?;
    }
    Ok(())
}

fn lock_schedule(config: &Config) -> Result<ScheduleLock> {
    private_dir(&config.root, config.uid, true)?;
    let path = config.root.join("lock");
    if exists(&path)? {
        private(&path)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(ScheduleLock(file)),
        Err(std::fs::TryLockError::WouldBlock) => Err("schedule_lock_busy".into()),
        Err(error) => Err(error.into()),
    }
}

fn reload_locked(config: &Config) -> Result<Config> {
    let current = read_config(&config.path)?;
    require(current.root == config.root, "schedule_config_drift")?;
    Ok(current)
}

fn interval_idle(state: &Value) -> Result<()> {
    let cycle = &state["cycle"];
    require(
        cycle.is_null() || terminal(cycle),
        "schedule_interval_busy: finish or reconcile the pending cycle before changing interval",
    )?;
    if let Some(attempts) = cycle["attempts"].as_array() {
        for attempt in attempts {
            require(
                !matches!(attempt["status"].as_str(), Some("running" | "pending")),
                "schedule_interval_busy: an attempt is unresolved",
            )?;
            if attempt["child_reaped"] != true && !attempt["child_pgid"].is_null() {
                let pgid = attempt["child_pgid"]
                    .as_u64()
                    .ok_or("schedule_interval_busy: child identity unresolved")?;
                require(
                    !group_alive(pgid)?,
                    "schedule_interval_busy: a child process group is still present",
                )?;
            }
        }
    }
    Ok(())
}

fn interval_config(config: &Config, seconds: u64) -> Result<Config> {
    require(
        (1..=31_536_000).contains(&seconds),
        "schedule_interval_seconds: --seconds must be 1..31536000",
    )?;
    for key in ["cycle_timeout_seconds", "retry_delay_seconds"] {
        let minimum = number(&config.value, key)?;
        require(
            seconds >= minimum,
            &format!(
                "schedule_interval_below_{key}: --seconds must be >= {minimum}; {key} is not changed"
            ),
        )?;
    }
    let mut value = config.value.clone();
    value["interval_seconds"] = json!(seconds);
    config_value(&config.path, value)
}

fn interval_anchor(config: &Config, state: &Value) -> Result<u64> {
    if let Some(anchor) = config.value["start_at_ms"].as_u64() {
        require(
            state.get("anchor_at_ms").is_none() || state["anchor_at_ms"] == anchor,
            "schedule_anchor_drift",
        )?;
        return Ok(anchor);
    }
    if let Some(anchor) = state["anchor_at_ms"].as_u64() {
        return Ok(anchor);
    }
    match number(state, "sequence")? {
        0 => number(state, "next_due_at_ms"),
        1 => number(&state["cycle"], "due_at_ms"),
        _ => {
            // Legacy states did not store the anchor. The first archived cycle
            // retains its original due time; never guess from the latest cycle.
            let cycles = config.root.join("cycles");
            private_dir(&cycles, config.uid, false)?;
            let mut anchors = Vec::new();
            for entry in fs::read_dir(cycles)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(suffix) = name.strip_prefix("00000001-") else {
                    continue;
                };
                private_dir(&entry.path(), config.uid, false)?;
                let first = load(private(entry.path().join("cycle.json"))?)?;
                let anchor: u64 = suffix.parse()?;
                require(
                    first["id"] == name && first["due_at_ms"] == anchor && terminal(&first),
                    "schedule_anchor_history_invalid",
                )?;
                anchors.push(anchor);
            }
            require(
                anchors.len() == 1,
                "schedule_anchor_unavailable: retain the first cycle archive",
            )?;
            Ok(anchors[0])
        }
    }
}

fn interval_state(old: &Config, new: &Config, prior: &Value, now: u64) -> Result<Value> {
    validate_state(old, prior)?;
    interval_idle(prior)?;
    require(
        now >= number(prior, "observed_at_ms")?,
        "schedule_clock_rollback",
    )?;
    let anchor = interval_anchor(old, prior)?;
    let due = if now < anchor {
        anchor
    } else {
        next_slot(anchor, number(&new.value, "interval_seconds")? * 1000, now)?
    };
    let mut next = prior.clone();
    next["config_sha256"] = json!(new.hash);
    next["anchor_at_ms"] = json!(anchor);
    next["next_due_at_ms"] = json!(due);
    next["observed_at_ms"] = json!(now);
    validate_state(new, &next)?;
    Ok(next)
}

fn interval_journal(config: &Config, seconds: u64, now: u64) -> Result<Value> {
    let new = interval_config(config, seconds)?;
    let prior = read_state(config, now)?;
    let next = interval_state(config, &new, &prior, now)?;
    let old_state = if exists(&config.root.join("state.json"))? {
        prior
    } else {
        Value::Null
    };
    Ok(
        json!({"protocol":"oo.library.refresh-interval-change/v1","config_path":config.path,
        "changed_at_ms":now,"old_config_sha256":config.hash,"new_config_sha256":new.hash,
        "old_config":config.value,"new_config":new.value,"old_state":old_state,"new_state":next}),
    )
}

// Caller holds the SAME lock as run-once. Only the exact recorded old/new pair
// is accepted, so a journal is not a general bypass for configuration drift.
fn recover_interval_locked(config: &Config) -> Result<Config> {
    let path = config.root.join(INTERVAL_JOURNAL);
    let journal = load(private(&path)?)?;
    fields(
        &journal,
        &[
            "protocol",
            "config_path",
            "changed_at_ms",
            "old_config_sha256",
            "new_config_sha256",
            "old_config",
            "new_config",
            "old_state",
            "new_state",
        ],
        &[],
    )?;
    require(
        journal["protocol"] == "oo.library.refresh-interval-change/v1"
            && journal["config_path"] == json!(config.path),
        "schedule_interval_journal_scope",
    )?;
    let old = config_value(&config.path, journal["old_config"].clone())?;
    let new = interval_config(&old, number(&journal["new_config"], "interval_seconds")?)?;
    require(
        old.root == config.root
            && new.value == journal["new_config"]
            && journal["old_config_sha256"] == old.hash
            && journal["new_config_sha256"] == new.hash
            && old.hash != new.hash,
        "schedule_interval_journal_drift",
    )?;
    let changed = number(&journal, "changed_at_ms")?;
    let prior = if journal["old_state"].is_null() {
        initial(&old, changed)
    } else {
        journal["old_state"].clone()
    };
    require(
        interval_state(&old, &new, &prior, changed)? == journal["new_state"],
        "schedule_interval_journal_state_drift",
    )?;
    let disk = load(private(&config.path)?)?;
    let state_path = config.root.join("state.json");
    let disk_state = if exists(&state_path)? {
        load(private(&state_path)?)?
    } else {
        Value::Null
    };
    if journal["old_state"].is_null() {
        require(
            !exists(&config.root.join("cycles"))?,
            "schedule_state_missing",
        )?;
    }
    require(
        (disk == old.value && disk_state == journal["old_state"])
            || (disk == new.value
                && (disk_state == journal["old_state"] || disk_state == journal["new_state"])),
        "schedule_interval_recovery_drift: config/state do not match the pending old/new pair",
    )?;
    if disk != new.value {
        persist(&config.path, &new.value)?;
    }
    if disk_state != journal["new_state"] {
        save_state(&new, &journal["new_state"])?;
    }
    fs::remove_file(path)?;
    File::open(&config.root)?.sync_all()?;
    Ok(new)
}

fn interval_recovered(config: &Config, now: u64) -> Result<Value> {
    report(
        config,
        read_state(config, now)?,
        "interval_recovered",
        true,
        now,
    )
}

fn set_interval(config: &Config, seconds: u64, now: u64) -> Result<Value> {
    if !exists(&config.root.join(INTERVAL_JOURNAL))? {
        interval_config(config, seconds)?;
    }
    let _lock = lock_schedule(config)?;
    let config = reload_locked(config)?;
    if exists(&config.root.join(INTERVAL_JOURNAL))? {
        return interval_recovered(&recover_interval_locked(&config)?, now);
    }
    let prior = read_state(&config, now)?;
    interval_idle(&prior)?;
    interval_config(&config, seconds)?;
    require(
        now >= number(&prior, "observed_at_ms")?,
        "schedule_clock_rollback",
    )?;
    if config.value["interval_seconds"] == seconds {
        return report(&config, prior, "interval_unchanged", true, now);
    }
    let journal = interval_journal(&config, seconds, now)?;
    persist(&config.root.join(INTERVAL_JOURNAL), &journal)?;
    let new = recover_interval_locked(&config)?;
    let mut result = report(&new, read_state(&new, now)?, "interval_changed", true, now)?;
    result["previous_interval_seconds"] = config.value["interval_seconds"].clone();
    result["interval_seconds"] = json!(seconds);
    Ok(result)
}

fn terminal(cycle: &Value) -> bool {
    matches!(
        cycle["status"].as_str(),
        Some("succeeded" | "failed" | "expired")
    )
}

fn cycle_dir(config: &Config, cycle: &Value) -> Result<PathBuf> {
    Ok(config.root.join("cycles").join(text(cycle, "id")?))
}

fn new_cycle(config: &Config, state: &Value, now: u64) -> Result<Value> {
    let sequence = number(state, "sequence")?
        .checked_add(1)
        .ok_or("schedule_overflow")?;
    let due = number(state, "next_due_at_ms")?;
    Ok(
        json!({"id":format!("{sequence:08}-{due}"), "due_at_ms":due, "started_at_ms":now,
        "deadline_at_ms":now.checked_add(number(&config.value, "cycle_timeout_seconds")? * 1000)
            .ok_or("schedule_overflow")?, "status":"running", "attempts":[],
        "next_attempt_at_ms":now}),
    )
}

fn render_string(config: &Config, cycle: &Value, value: &str) -> Result<String> {
    let dir = cycle_dir(config, cycle)?;
    let replacements = [
        (
            "{cycle_dir}",
            dir.to_str().ok_or("schedule_path")?.to_owned(),
        ),
        ("{cycle_id}", text(cycle, "id")?.to_owned()),
        (
            "{started_at_ms}",
            number(cycle, "started_at_ms")?.to_string(),
        ),
        (
            "{deadline_at_ms}",
            number(cycle, "deadline_at_ms")?.to_string(),
        ),
        (
            "{config_dir}",
            config.directory.to_str().ok_or("schedule_path")?.to_owned(),
        ),
    ];
    let mut value = value.to_owned();
    for (key, replacement) in &replacements {
        value = value.replace(key, replacement);
    }
    Ok(value)
}

fn render(config: &Config, cycle: &Value, resume: bool) -> Result<Vec<String>> {
    config.value["command"][if resume { "resume_argv" } else { "argv" }]
        .as_array()
        .ok_or("schedule_argv")?
        .iter()
        .map(|arg| render_string(config, cycle, arg.as_str().ok_or("schedule_argv")?))
        .collect()
}

fn render_json(config: &Config, cycle: &Value, value: &mut Value) -> Result<()> {
    match value {
        Value::String(string) => *string = render_string(config, cycle, string)?,
        Value::Object(object) => {
            for child in object.values_mut() {
                render_json(config, cycle, child)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                render_json(config, cycle, child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn template_snapshot(config: &Config, cycle: &Value) -> Result<Option<Value>> {
    let Some(templates) = config.value["templates"].as_object() else {
        return Ok(None);
    };
    let dir = cycle_dir(config, cycle)?;
    let path = dir.join("templates.snapshot.json");
    let snapshot = if exists(&path)? {
        load(private(&path)?)?
    } else {
        let mut files = serde_json::Map::new();
        for (name, source) in templates {
            let mut value = load(private(
                config
                    .directory
                    .join(source.as_str().ok_or("schedule_template_source")?),
            )?)?;
            render_json(config, cycle, &mut value)?;
            files.insert(name.clone(), value);
        }
        json!({"config_sha256":config.hash,"cycle_dir":dir,"files":files})
    };
    require(
        snapshot["config_sha256"] == config.hash
            && snapshot["cycle_dir"] == json!(dir)
            && snapshot["files"].as_object().is_some_and(|files| {
                files.len() == templates.len()
                    && templates.keys().all(|key| files.contains_key(key))
            }),
        "schedule_template_snapshot_drift",
    )?;
    Ok(Some(snapshot))
}

fn materialize_templates(config: &Config, cycle: &Value) -> Result<()> {
    let Some(snapshot) = template_snapshot(config, cycle)? else {
        return Ok(());
    };
    let dir = cycle_dir(config, cycle)?;
    let path = dir.join("templates.snapshot.json");
    if !exists(&path)? {
        persist(&path, &snapshot)?;
    }
    for (name, value) in snapshot["files"].as_object().ok_or("schedule_templates")? {
        let path = dir.join(name);
        if exists(&path)? {
            private(&path)?;
            require(load(&path)? == *value, "schedule_template_output_drift")?;
        } else {
            persist(&path, value)?;
        }
    }
    File::open(dir)?.sync_all()?;
    Ok(())
}

fn save_state(config: &Config, state: &Value) -> Result<()> {
    persist(&config.root.join("state.json"), state)
}

fn next_slot(due: u64, interval_ms: u64, now: u64) -> Result<u64> {
    let slots = now.saturating_sub(due) / interval_ms + 1;
    due.checked_add(slots.checked_mul(interval_ms).ok_or("schedule_overflow")?)
        .ok_or_else(|| "schedule_overflow".into())
}

fn finish(config: &Config, state: &mut Value, status: &str, now: u64, reason: &str) -> Result<()> {
    state["cycle"]["status"] = json!(status);
    state["cycle"]["finished_at_ms"] = json!(now);
    state["cycle"]["reason"] = json!(reason);
    let field = if status == "succeeded" {
        "last_success"
    } else {
        "last_failure"
    };
    state[field] = json!({"cycle_id":state["cycle"]["id"],"at_ms":now,"reason":reason});
    state["next_due_at_ms"] = json!(next_slot(
        number(&state["cycle"], "due_at_ms")?,
        number(&config.value, "interval_seconds")? * 1000,
        now
    )?);
    save_state(config, state)
}

fn report(config: &Config, state: Value, action: &str, ok: bool, now: u64) -> Result<Value> {
    let cycle = &state["cycle"];
    let wake = if cycle.is_null() || terminal(cycle) {
        number(&state, "next_due_at_ms")?
    } else {
        number(cycle, "next_attempt_at_ms")?.min(number(cycle, "deadline_at_ms")?)
    };
    Ok(
        json!({"protocol":"oo.library.refresh-schedule-status/v1","ok":ok,"action":action,
        "state_dir":config.root,"next_wake_at_ms":wake,"due":now >= wake,
        "clock_rollback":now < number(&state, "observed_at_ms")?, "state":state}),
    )
}

fn inspect(config: &Config, dry_run: bool, now: u64) -> Result<Value> {
    if exists(&config.root.join(INTERVAL_JOURNAL))? {
        let _lock = lock_schedule(config)?;
        let current = reload_locked(config)?;
        let current = if exists(&current.root.join(INTERVAL_JOURNAL))? {
            recover_interval_locked(&current)?
        } else {
            current
        };
        return interval_recovered(&current, now);
    }
    let state = read_state(config, now)?;
    let path = config.root.join("lock");
    let locked = if exists(&path)? {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(private(path)?)?;
        match lock.try_lock() {
            Ok(()) => {
                let _lock = ScheduleLock(lock);
                false
            }
            Err(std::fs::TryLockError::WouldBlock) => true,
            Err(error) => return Err(error.into()),
        }
    } else {
        false
    };
    let mut result = report(
        config,
        state.clone(),
        if dry_run { "dry_run" } else { "status" },
        true,
        now,
    )?;
    result["locked"] = json!(locked);
    if dry_run {
        let fresh = state["cycle"].is_null() || terminal(&state["cycle"]);
        let cycle = if fresh {
            new_cycle(config, &state, now.max(number(&state, "next_due_at_ms")?))?
        } else {
            state["cycle"].clone()
        };
        let resume = !cycle["attempts"]
            .as_array()
            .ok_or("schedule_attempts")?
            .is_empty();
        let recovery_check = cycle["attempts"]
            .as_array()
            .and_then(|a| a.last())
            .is_some_and(|last| last["child_pgid"].is_number() && last["child_reaped"] != true);
        let templates = template_snapshot(config, &cycle)?;
        result["plan"] = json!({"cycle":cycle,"resume":resume,"argv":render(config,&cycle,resume)?,
            "templates":templates.map(|v| v["files"].as_object().unwrap().keys().cloned().collect::<Vec<_>>()),
            "recovery_check_required":recovery_check,
            "will_execute_now":result["due"] == true && !locked && !recovery_check && result["clock_rollback"] == false
                && now < number(&cycle, "deadline_at_ms")?
                && cycle["attempts"].as_array().unwrap().len() < number(&config.value, "max_attempts")? as usize});
    }
    Ok(result)
}

// The gate prevents a crash between spawn and PID persistence from starting work.
// Only "$@" is executed: configuration arguments never become shell source.
const GATE: &str = "IFS= read -r gate || exit 125; [ \"$gate\" = run ] || exit 125; exec \"$@\"";

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // refresh stages create their own process groups. Stop only descendants
        // of this live child before killing its group; never select by name.
        if self.0.try_wait().ok().flatten().is_none() {
            if let Ok(output) = Command::new("/bin/ps")
                .args(["-axo", "pid=,ppid="])
                .output()
            {
                let rows: Vec<(u32, u32)> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter_map(|line| {
                        let mut words = line.split_whitespace();
                        Some((words.next()?.parse().ok()?, words.next()?.parse().ok()?))
                    })
                    .collect();
                let mut descendants = vec![self.0.id()];
                let mut index = 0;
                while index < descendants.len() {
                    for &(pid, parent) in &rows {
                        if parent == descendants[index] && !descendants.contains(&pid) {
                            descendants.push(pid);
                        }
                    }
                    index += 1;
                }
                // Parents first: killing a waiter's child first can wake its
                // shell and let the next (publishing) statement execute.
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", "--"])
                    .args(descendants.iter().map(u32::to_string))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
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

fn group_alive(pgid: u64) -> Result<bool> {
    require(
        pgid > 1 && pgid <= i32::MAX as u64,
        "schedule_child_identity",
    )?;
    let output = Command::new("/bin/ps").args(["-axo", "pgid="]).output()?;
    require(output.status.success(), "schedule_process_inventory")?;
    Ok(std::str::from_utf8(&output.stdout)?
        .lines()
        .any(|line| line.trim().parse::<u64>().ok() == Some(pgid)))
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn no_failure(value: &Value, refresh_history: bool) -> Result<()> {
    match value {
        Value::Object(object) => {
            require(
                !["error", "errors", "failed", "failure"]
                    .iter()
                    .any(|key| truthy(&value[*key]))
                    && !object
                        .iter()
                        .any(|(key, value)| key.ends_with("_error") && truthy(value))
                    && value["success"] != false
                    && value["ok"] != false
                    && !matches!(
                        value["status"].as_str(),
                        Some("failed" | "error" | "refused" | "pending" | "running" | "expired")
                    ),
                "schedule_structured_failure",
            )?;
            for (key, child) in object {
                if key == "attempts" && refresh_history {
                    let last = child
                        .as_array()
                        .and_then(|a| a.last())
                        .ok_or("schedule_receipt_attempts")?;
                    require(last["status"] == "succeeded", "schedule_receipt_incomplete")?;
                    no_failure(last, false)?;
                } else {
                    no_failure(child, refresh_history)?;
                }
            }
        }
        Value::Array(array) => {
            for child in array {
                no_failure(child, refresh_history)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn success_output(path: &Path) -> Result<Value> {
    require(
        fs::metadata(path)?.len() <= 4 * 1024 * 1024,
        "schedule_stdout_budget",
    )?;
    let value = load(private(path)?)?;
    require(
        value.is_object() && value["success"] == true,
        "schedule_success_required",
    )?;
    if value["protocol"] == "oo.library.refresh-result/v1" {
        let stages = [
            "prepare",
            "capture",
            "verify",
            "publish",
            "probe",
            "activate",
            "final_probe",
        ];
        fields(&value["stages"], &stages, &[])?;
        for name in stages {
            let stage = &value["stages"][name];
            let last = stage["attempts"]
                .as_array()
                .and_then(|a| a.last())
                .ok_or("schedule_receipt_attempts")?;
            require(
                stage["status"] == "succeeded"
                    && last["status"] == "succeeded"
                    && last["exit_code"] == 0,
                "schedule_receipt_incomplete",
            )?;
        }
    } else if value["protocol"] == "oo.library.incremental-refresh/v1" {
        require(
            value["records"]
                .as_object()
                .is_some_and(|records| !records.is_empty()),
            "schedule_receipt_incomplete",
        )?;
    }
    no_failure(
        &value,
        matches!(
            value["protocol"].as_str(),
            Some("oo.library.refresh-result/v1" | "oo.library.incremental-refresh/v1")
        ),
    )?;
    Ok(json!({"sha256":sha(&fs::read(path)?),"bytes":fs::metadata(path)?.len()}))
}

fn check_stderr(path: &Path) -> Result<()> {
    let raw = fs::read_to_string(path)?;
    let (mut offset, mut end) = (0, 0);
    for line in raw.split_inclusive('\n') {
        let start = offset + line.len() - line.trim_start().len();
        if start >= end && matches!(raw.as_bytes().get(start), Some(b'{' | b'[')) {
            let mut stream = serde_json::Deserializer::from_str(&raw[start..]).into_iter::<Value>();
            if let Some(Ok(value)) = stream.next() {
                end = start + stream.byte_offset();
                no_failure(&value, false)?;
            }
        }
        offset += line.len();
    }
    Ok(())
}

fn execute(
    config: &Config,
    state: &mut Value,
    clock: &mut impl FnMut() -> Result<u64>,
) -> Result<()> {
    let now = clock()?;
    require(
        now >= number(state, "observed_at_ms")?,
        "schedule_clock_rollback",
    )?;
    let deadline_ms = number(&state["cycle"], "deadline_at_ms")?;
    require(now < deadline_ms, "schedule_deadline_exceeded")?;
    let deadline = Instant::now() + Duration::from_millis(deadline_ms - now);
    let cycle = &state["cycle"];
    let attempts = cycle["attempts"]
        .as_array()
        .ok_or("schedule_attempts")?
        .len();
    let resume = attempts > 0;
    let argv = render(config, cycle, resume)?;
    let dir = cycle_dir(config, cycle)?;
    let attempt_dir = dir.join(format!("attempt-{:04}", attempts + 1));
    let stdout_path = attempt_dir.join("stdout.json");
    let attempt = json!({"status":"running","started_at_ms":now,"resume":resume,
        "directory":attempt_dir.file_name().unwrap().to_str().ok_or("schedule_path")?,
        "child_pgid":null,"exit_code":null});
    state["cycle"]["attempts"]
        .as_array_mut()
        .ok_or("schedule_attempts")?
        .push(attempt);
    state["cycle"]["status"] = json!("running");
    state["observed_at_ms"] = json!(now);
    save_state(config, state)?;
    // Reserve the attempt first so a crash during log creation never reuses files.
    fs::DirBuilder::new().mode(0o700).create(&attempt_dir)?;
    let stdout = open_new(&stdout_path)?;
    let stderr = open_new(&attempt_dir.join("stderr.log"))?;
    let outcome = (|| -> Result<Value> {
        materialize_templates(config, &state["cycle"])?;
        let mut child = OwnedChild(
            Command::new("/bin/sh")
                .args(["-c", GATE, "refresh-schedule"])
                .args(&argv)
                .current_dir(&config.directory)
                .env("OO_REFRESH_CYCLE_DIR", &dir)
                .env("OO_REFRESH_CYCLE_ID", text(&state["cycle"], "id")?)
                .env(
                    "OO_REFRESH_STARTED_AT_MS",
                    number(&state["cycle"], "started_at_ms")?.to_string(),
                )
                .env("OO_REFRESH_DEADLINE_AT_MS", deadline_ms.to_string())
                .env("OO_REFRESH_RESUME", if resume { "1" } else { "0" })
                .stdin(Stdio::piped())
                .stdout(stdout.try_clone()?)
                .stderr(stderr.try_clone()?)
                .process_group(0)
                .spawn()?,
        );
        state["cycle"]["attempts"][attempts]["child_pgid"] = json!(child.0.id());
        save_state(config, state)?;
        let before_gate = clock()?;
        require(
            before_gate >= number(state, "observed_at_ms")?,
            "schedule_clock_rollback",
        )?;
        require(
            before_gate < deadline_ms && Instant::now() < deadline,
            "schedule_deadline_exceeded",
        )?;
        child
            .0
            .stdin
            .take()
            .ok_or("schedule_child_gate")?
            .write_all(b"run\n")?;
        loop {
            let now = clock()?;
            let observed = number(state, "observed_at_ms")?;
            require(now >= observed, "schedule_clock_rollback")?;
            require(
                now < deadline_ms && Instant::now() < deadline,
                "schedule_deadline_exceeded",
            )?;
            require(
                stdout.metadata()?.len() <= 4 * 1024 * 1024
                    && stderr.metadata()?.len() <= 16 * 1024 * 1024,
                "schedule_log_budget",
            )?;
            if now.saturating_sub(observed) >= 1000 {
                state["observed_at_ms"] = json!(now);
                save_state(config, state)?;
            }
            if let Some(status) = child.0.try_wait()? {
                state["cycle"]["attempts"][attempts]["exit_code"] = json!(status.code());
                require(status.success(), "schedule_child_exit_nonzero")?;
                // A foreground command must not report success while its group works on.
                require(
                    !group_alive(child.0.id().into())?,
                    "schedule_child_descendants",
                )?;
                stdout.sync_all()?;
                check_stderr(&attempt_dir.join("stderr.log"))?;
                return success_output(&stdout_path);
            }
            std::thread::sleep(
                Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    })();
    stdout.sync_all()?;
    stderr.sync_all()?;
    File::open(&attempt_dir)?.sync_all()?;
    let now = clock()?;
    let outcome = outcome.and_then(|proof| {
        require(
            now >= number(state, "observed_at_ms")?,
            "schedule_clock_rollback",
        )?;
        require(
            now < deadline_ms && Instant::now() < deadline,
            "schedule_deadline_exceeded",
        )?;
        Ok(proof)
    });
    state["observed_at_ms"] = json!(now.max(number(state, "observed_at_ms")?));
    let attempt = &mut state["cycle"]["attempts"][attempts];
    attempt["finished_at_ms"] = json!(now);
    if let Some(pgid) = attempt["child_pgid"].as_u64() {
        attempt["child_reaped"] = json!(!group_alive(pgid)?);
    }
    match outcome {
        Ok(proof) => {
            attempt["status"] = json!("succeeded");
            attempt["stdout"] = proof;
            finish(config, state, "succeeded", now, "command_succeeded")
        }
        Err(error) => {
            let reason = error.to_string();
            attempt["status"] = json!("failed");
            attempt["error"] = json!(reason);
            state["last_failure"] =
                json!({"cycle_id":state["cycle"]["id"],"at_ms":now,"reason":reason});
            if now >= deadline_ms || reason == "schedule_deadline_exceeded" {
                finish(config, state, "expired", now, &reason)
            } else if attempts + 1 >= number(&config.value, "max_attempts")? as usize {
                finish(config, state, "failed", now, &reason)
            } else {
                state["cycle"]["status"] = json!("retry_wait");
                state["cycle"]["next_attempt_at_ms"] = json!(
                    now.checked_add(number(&config.value, "retry_delay_seconds")? * 1000)
                        .ok_or("schedule_overflow")?
                );
                save_state(config, state)
            }
        }
    }
}

fn run_once(config: &Config, clock: &mut impl FnMut() -> Result<u64>) -> Result<Value> {
    let _lock = lock_schedule(config)?;
    let current = reload_locked(config)?;
    let config = &current;
    let now = clock()?;
    if exists(&config.root.join(INTERVAL_JOURNAL))? {
        return interval_recovered(&recover_interval_locked(config)?, now);
    }
    let mut state = read_state(config, now)?;
    require(
        now >= number(&state, "observed_at_ms")?,
        "schedule_clock_rollback",
    )?;
    state["observed_at_ms"] = json!(now);
    // Even an expired cycle cannot overlap a surviving child after a parent crash.
    if let Some(last) = state["cycle"]["attempts"].as_array().and_then(|a| a.last()) {
        if let Some(pgid) = last["child_pgid"]
            .as_u64()
            .filter(|_| last["child_reaped"] != true)
        {
            if group_alive(pgid)? {
                if !terminal(&state["cycle"]) && now >= number(&state["cycle"], "deadline_at_ms")? {
                    finish(
                        config,
                        &mut state,
                        "expired",
                        now,
                        "schedule_child_survived_deadline",
                    )?;
                }
                save_state(config, &state)?;
                return report(config, state, "child_still_running", false, now);
            }
            let attempts = state["cycle"]["attempts"]
                .as_array_mut()
                .ok_or("schedule_attempts")?;
            let last = attempts.last_mut().ok_or("schedule_attempts")?;
            last["child_reaped"] = json!(true);
            if last["status"] == "running" {
                last["status"] = json!("interrupted");
                last["finished_at_ms"] = json!(now);
                state["last_failure"] = json!({"cycle_id":state["cycle"]["id"],"at_ms":now,
                    "reason":"schedule_interrupted"});
            }
        }
    }
    if !state["cycle"].is_null() && !terminal(&state["cycle"]) {
        let count = state["cycle"]["attempts"]
            .as_array()
            .ok_or("schedule_attempts")?
            .len();
        if count > 0 && state["cycle"]["attempts"][count - 1]["status"] == "running" {
            state["cycle"]["attempts"][count - 1]["status"] = json!("interrupted");
            state["cycle"]["attempts"][count - 1]["finished_at_ms"] = json!(now);
            state["last_failure"] = json!({"cycle_id":state["cycle"]["id"],"at_ms":now,
                "reason":"schedule_interrupted"});
        }
        if now >= number(&state["cycle"], "deadline_at_ms")? {
            finish(
                config,
                &mut state,
                "expired",
                now,
                "schedule_deadline_exceeded",
            )?;
            return report(config, state, "expired", false, now);
        }
        if count >= number(&config.value, "max_attempts")? as usize {
            finish(
                config,
                &mut state,
                "failed",
                now,
                "schedule_attempts_exhausted",
            )?;
            return report(config, state, "failed", false, now);
        }
        if now < number(&state["cycle"], "next_attempt_at_ms")? {
            save_state(config, &state)?;
            return report(config, state, "retry_not_due", true, now);
        }
    } else {
        if now < number(&state, "next_due_at_ms")? {
            save_state(config, &state)?;
            return report(config, state, "not_due", true, now);
        }
        if !state["cycle"].is_null() {
            private_dir(&config.root.join("cycles"), config.uid, false)?;
            private_dir(&cycle_dir(config, &state["cycle"])?, config.uid, false)?;
            persist(
                &cycle_dir(config, &state["cycle"])?.join("cycle.json"),
                &state["cycle"],
            )?;
        }
        state["cycle"] = new_cycle(config, &state, now)?;
        state["sequence"] = json!(number(&state, "sequence")? + 1);
        state["next_due_at_ms"] = json!(next_slot(
            number(&state["cycle"], "due_at_ms")?,
            number(&config.value, "interval_seconds")? * 1000,
            now
        )?);
        save_state(config, &state)?;
    }
    private_dir(&config.root.join("cycles"), config.uid, true)?;
    private_dir(&cycle_dir(config, &state["cycle"])?, config.uid, true)?;
    execute(config, &mut state, clock)?;
    let action = text(&state["cycle"], "status")?.to_owned();
    report(config, state, &action, action == "succeeded", clock()?)
}

pub fn dispatch(args: &[String]) -> Result<()> {
    if args.is_empty()
        || args == ["--help"]
        || args == ["help"]
        || (args.len() == 2
            && args[1] == "--help"
            && ["run-once", "status", "dry-run", "set-interval"].contains(&args[0].as_str()))
    {
        print!("{HELP}");
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "set-interval") {
        require(
            args.len() == 5 && args[1] == "--config" && args[3] == "--seconds",
            "usage: refresh-schedule set-interval --config PRIVATE_CONFIG --seconds N",
        )?;
        let seconds = args[4]
            .parse()
            .map_err(|_| "schedule_interval_seconds: --seconds must be an integer")?;
        let config = read_config(Path::new(&args[2]))?;
        println!("{}", set_interval(&config, seconds, wall_ms()?)?);
        return Ok(());
    }
    require(
        args.len() == 3
            && args[1] == "--config"
            && ["run-once", "status", "dry-run"].contains(&args[0].as_str()),
        "usage: refresh-schedule run-once|status|dry-run|set-interval --config PRIVATE_SCHEDULE (see --help)",
    )?;
    let config = read_config(Path::new(&args[2]))?;
    let result = match args[0].as_str() {
        "run-once" => run_once(&config, &mut wall_ms)?,
        "status" => inspect(&config, false, wall_ms()?)?,
        _ => inspect(&config, true, wall_ms()?)?,
    };
    println!("{result}");
    require(result["ok"] == true, "schedule_attempt_incomplete")
}

#[cfg(test)]
#[path = "refresh_schedule_tests.rs"]
mod tests;
