//! Subprocess adapter for the compiled `query-pg` binary.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

use serde_json::{Map, Value};

use crate::produce::{Error, SelectRunner, Spawned};

const STDOUT_CAP: usize = 65_536;
const WAIT: Duration = Duration::from_secs(40);

pub struct ProcessRunner {
    bin: PathBuf,
    request: Value,
    dir: PathBuf,
    seq: u64,
}

impl ProcessRunner {
    pub fn open(config: &Value) -> Result<(Self, String, usize), Error> {
        let map = config.as_object().ok_or(Error::Malformed("type"))?;
        only(
            map,
            &[
                "schema",
                "query_pg_bin",
                "guard_probe_sql",
                "max_validated",
                "request",
            ],
        )?;
        let schema = required_str(map, "schema")?;
        if schema != "oo.selfbuild-suggestions.gateway-config/v1" {
            return Err(Error::Malformed("type"));
        }
        let bin = required_str(map, "query_pg_bin")?;
        if bin.is_empty() || bin.contains('\0') {
            return Err(Error::Malformed("config"));
        }
        let bin_meta = fs::symlink_metadata(bin).map_err(|_| Error::Malformed("config"))?;
        if !bin_meta.file_type().is_file() {
            return Err(Error::Malformed("config"));
        }
        let probe = required_str(map, "guard_probe_sql")?;
        if probe.chars().count() == 0
            || probe.chars().count() > 400
            || probe.contains(';')
            || probe.contains('\0')
        {
            return Err(Error::Malformed("type"));
        }
        let max = map
            .get("max_validated")
            .and_then(Value::as_number)
            .filter(|number| !number.is_f64())
            .and_then(|number| number.as_u64())
            .ok_or(Error::Malformed("type"))?;
        if !(1..=4).contains(&max) {
            return Err(Error::Malformed("type"));
        }
        let request = map.get("request").ok_or(Error::Malformed("type"))?;
        let request_map = request.as_object().ok_or(Error::Malformed("type"))?;
        let statement = request_map
            .get("statement")
            .and_then(Value::as_object)
            .ok_or(Error::Malformed("type"))?;
        if !statement.contains_key("sql") || !statement.contains_key("source") {
            return Err(Error::Malformed("type"));
        }
        let dir = private_dir()?;
        Ok((
            Self {
                bin: PathBuf::from(bin),
                request: request.clone(),
                dir,
                seq: 0,
            },
            probe.to_string(),
            usize::try_from(max).unwrap_or(1),
        ))
    }
}

impl Drop for ProcessRunner {
    fn drop(&mut self) {
        let name = self
            .dir
            .file_name()
            .and_then(|item| item.to_str())
            .unwrap_or("");
        if name.starts_with("oo-a355-sbs-") && self.dir.starts_with(std::env::temp_dir()) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

impl SelectRunner for ProcessRunner {
    fn run_select(&mut self, sql: &str) -> Spawned {
        self.seq += 1;
        let request_path = self.dir.join(format!("request-{}.json", self.seq));
        let audit_path = self.dir.join(format!("audit-{}.jsonl", self.seq));
        let bytes = match rewrite(&self.request, sql, &audit_path) {
            Ok(bytes) => bytes,
            Err(()) => return Spawned::not_started(),
        };
        if write_private(&request_path, &bytes).is_err() {
            return Spawned::not_started();
        }
        let mut command = Command::new(&self.bin);
        command
            .arg("--request")
            .arg(&request_path)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/tmp")
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => return Spawned::not_started(),
        };
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let out_thread = thread::spawn(move || match stdout {
            Some(pipe) => read_capped(pipe),
            None => Vec::new(),
        });
        let err_thread = thread::spawn(move || match stderr {
            Some(pipe) => read_capped(pipe),
            None => Vec::new(),
        });
        let started = Instant::now();
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if started.elapsed() >= WAIT => {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait().ok();
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => break None,
            }
        };
        let stdout = out_thread.join().unwrap_or_default();
        let _stderr = err_thread.join().unwrap_or_default();
        Spawned {
            exit: status.and_then(|item| item.code()),
            stdout,
            timed_out,
            started: true,
        }
    }
}

fn rewrite(template: &Value, sql: &str, audit: &Path) -> Result<Vec<u8>, ()> {
    let mut doc = template.clone();
    let obj = doc.as_object_mut().ok_or(())?;
    let statement = obj
        .get_mut("statement")
        .and_then(Value::as_object_mut)
        .ok_or(())?;
    statement.insert("sql".to_string(), Value::String(sql.to_string()));
    statement.insert("source".to_string(), Value::String("request".to_string()));
    let audit_text = audit.to_str().ok_or(())?;
    obj.insert(
        "audit_file".to_string(),
        Value::String(audit_text.to_string()),
    );
    serde_json::to_vec(&doc).map_err(|_| ())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), ()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| ())?;
    file.write_all(bytes).map_err(|_| ())?;
    Ok(())
}

fn read_capped(mut pipe: impl Read) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(size) => {
                if kept.len() < STDOUT_CAP {
                    let room = STDOUT_CAP - kept.len();
                    let take = size.min(room);
                    kept.extend_from_slice(&buf[..take]);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    kept
}

fn private_dir() -> Result<PathBuf, Error> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Malformed("io"))?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("oo-a355-sbs-{}-{nanos}", std::process::id()));
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    builder.create(&dir).map_err(|_| Error::Malformed("io"))?;
    Ok(dir)
}

fn only(map: &Map<String, Value>, allowed: &[&str]) -> Result<(), Error> {
    if map
        .keys()
        .any(|key| !allowed.iter().any(|item| item == key))
    {
        return Err(Error::Malformed("unknown_field"));
    }
    Ok(())
}

fn required_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, Error> {
    map.get(key)
        .and_then(Value::as_str)
        .ok_or(Error::Malformed("type"))
}
