//! Process and evidence helpers. Passwords are not written here.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Stop {
    pub step: String,
    pub detail: String,
}

impl Stop {
    pub fn new(step: &str, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        let detail = if detail.len() > 400 {
            format!("{}…", &detail[..400])
        } else {
            detail
        };
        Self {
            step: step.to_string(),
            detail,
        }
    }
}

pub struct Spawn {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn evidence() -> Result<PathBuf, Stop> {
    env::var("OO_A259_EVIDENCE")
        .map(PathBuf::from)
        .map_err(|_| Stop::new("env", "OO_A259_EVIDENCE"))
}

pub fn env_var(name: &str) -> Result<String, Stop> {
    env::var(name).map_err(|_| Stop::new("env", name))
}

pub fn read_hex(path: &str) -> Result<String, Stop> {
    let text = fs::read_to_string(path).map_err(|_| Stop::new("secret_file", "unreadable"))?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(Stop::new("secret_file", "password file was rejected"));
    }
    Ok(trimmed)
}

pub fn spawn(program: &Path, args: &[&str], stdin_text: &[u8]) -> Result<Spawn, Stop> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Stop::new("spawn", program.display().to_string()))?;
    child
        .stdin
        .take()
        .ok_or_else(|| Stop::new("spawn", "stdin"))?
        .write_all(stdin_text)
        .map_err(|_| Stop::new("spawn", "write"))?;
    let output = child.wait_with_output().map_err(|_| Stop::new("spawn", "wait"))?;
    Ok(Spawn {
        code: output.status.code().unwrap_or(127),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

pub fn bin(env_name: &str) -> Result<PathBuf, Stop> {
    env_var(env_name).map(PathBuf::from)
}

pub fn run_ok(env_name: &str, args: &[&str], stdin_text: &str) -> Result<String, Stop> {
    let program = bin(env_name)?;
    let output = spawn(&program, args, stdin_text.as_bytes())?;
    if output.code != 0 {
        return Err(Stop::new(
            env_name,
            format!(
                "exit {} {}",
                output.code,
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| Stop::new(env_name, "stdout utf-8"))
}

pub fn save_step(dir: &Path, name: &str, output: &Spawn) -> Result<(), Stop> {
    fs::create_dir_all(dir).map_err(|_| Stop::new(name, "mkdir"))?;
    fs::write(dir.join(format!("{name}.stdout")), &output.stdout).map_err(|_| Stop::new(name, "stdout"))?;
    fs::write(dir.join(format!("{name}.stderr")), &output.stderr).map_err(|_| Stop::new(name, "stderr"))?;
    fs::write(dir.join(format!("{name}.exit")), format!("{}\n", output.code)).map_err(|_| Stop::new(name, "exit"))?;
    Ok(())
}

pub fn write_stop(stop: &Stop) {
    let root = env::var("OO_A259_EVIDENCE").unwrap_or_else(|_| "/evidence".to_string());
    let body = format!(
        "{}\n",
        serde_json::json!({"step": stop.step, "detail": stop.detail})
    );
    let _ = fs::write(format!("{root}/stop.json"), body);
    eprintln!("stop step={} detail={}", stop.step, stop.detail);
}

pub fn reject_secrets(bytes: &[u8], secrets: &[&str]) -> Result<(), Stop> {
    for secret in secrets {
        if secret.is_empty() {
            continue;
        }
        if find_subslice(bytes, secret.as_bytes()).is_some() {
            return Err(Stop::new("secret", "output contained a password"));
        }
    }
    Ok(())
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|window| window == needle)
}

pub fn utf8(step: &str, bytes: &[u8]) -> Result<String, Stop> {
    String::from_utf8(bytes.to_vec()).map_err(|_| Stop::new(step, "utf-8"))
}
