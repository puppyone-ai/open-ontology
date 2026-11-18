//! The document parser sidecar without Docker (D83, ADR-S14 step 2): a
//! uv-managed CPython environment that already holds the pinned packages
//! (`venv/`), the parser package (`app/`) and the model files (`models/`),
//! started by `oo serve` as a loopback child of the embedded runtime. Nothing is
//! downloaded here: when the environment is not there, Office and PDF files stay
//! `unsupported` and `oo docs wait` / `oo docs upload` say why.
//!
//! Layout under the sidecar directory (`OO_DOCS_SIDECAR_DIR`, default
//! `<state>/sidecar`), as `tools/oo-test/super_ontology_nodocker.py --sidecar` builds it:
//! `venv/bin/python`, `app/doc_parser/`, `app/models.lock.json`, `models/`.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::secrets;

/// A found environment.
#[derive(Debug, PartialEq, Eq)]
pub struct SidecarEnv {
    pub dir: PathBuf,
    pub python: PathBuf,
}

/// Where the environment should be.
pub fn env_dir(state: &Path) -> PathBuf {
    std::env::var_os("OO_DOCS_SIDECAR_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| state.join("sidecar"))
}

/// The environment in `dir`, or the first thing that is missing from it.
pub fn probe(dir: &Path) -> Result<SidecarEnv, String> {
    let python = dir.join("venv/bin/python");
    let missing = |what: &str| {
        format!(
            "no parser sidecar: {what} is missing in {} (build it with tools/oo-test/super_ontology_nodocker.py --sidecar, or set OO_DOCS_SIDECAR_DIR); docx, pdf and other Office files stay unsupported without Docker",
            dir.display()
        )
    };
    if !python.is_file() {
        return Err(missing("venv/bin/python"));
    }
    if !dir.join("app/doc_parser").is_dir() {
        return Err(missing("app/doc_parser"));
    }
    if !dir.join("models").is_dir() {
        return Err(missing("models/"));
    }
    Ok(SidecarEnv {
        dir: dir.to_path_buf(),
        python,
    })
}

/// A running sidecar; killed when dropped.
pub struct Sidecar {
    child: Child,
    pub url: String,
    pub token_file: PathBuf,
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.token_file);
    }
}

fn free_port() -> Option<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
    l.local_addr().ok().map(|a| a.port())
}

/// The environment of the sidecar process: nothing inherited, no network
/// settings, models offline.
pub fn child_env(
    env: &SidecarEnv,
    scratch: &Path,
    token_file: &Path,
    port: u16,
) -> Vec<(String, String)> {
    let s = |p: &Path| p.display().to_string();
    let bin = env.python.parent().map(s).unwrap_or_default();
    vec![
        ("PATH".into(), format!("{bin}:/usr/bin:/bin")),
        ("HOME".into(), s(scratch)),
        ("TMPDIR".into(), s(scratch)),
        ("OO_PARSER_TOKEN_FILE".into(), s(token_file)),
        ("OO_PARSER_HOST".into(), "127.0.0.1".into()),
        ("OO_PARSER_PORT".into(), port.to_string()),
        ("OO_PARSER_SCRATCH".into(), s(scratch)),
        (
            "OO_PARSER_MODELS_LOCK".into(),
            s(&env.dir.join("app/models.lock.json")),
        ),
        ("DOCLING_ARTIFACTS_PATH".into(), s(&env.dir.join("models"))),
        ("HF_HUB_OFFLINE".into(), "1".into()),
        ("TRANSFORMERS_OFFLINE".into(), "1".into()),
        ("HF_HUB_DISABLE_TELEMETRY".into(), "1".into()),
        ("DO_NOT_TRACK".into(), "1".into()),
        ("OMP_NUM_THREADS".into(), "2".into()),
        ("MKL_NUM_THREADS".into(), "2".into()),
        ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
        ("PYTHONUNBUFFERED".into(), "1".into()),
        // forkserver workers die on macOS arm64 once torch is imported (ADR-S14)
        ("OO_PARSER_START_METHOD".into(), "spawn".into()),
    ]
}

/// Start the sidecar and wait until it listens.
pub fn start(state: &Path, env: &SidecarEnv) -> Result<Sidecar, String> {
    let token = secrets::new_token("oodp_").map_err(|_| "no randomness source".to_owned())?;
    let token_file = state.join("parser.token");
    secrets::write_secret_file(&token_file, &token)
        .map_err(|_| "cannot write the parser token".to_owned())?;
    let scratch = state.join("sidecar-scratch");
    secrets::ensure_state_dir(&scratch).map_err(|e| e.0.to_owned())?;
    let port = free_port().ok_or("no free loopback port")?;
    let log = state.join("sidecar.log");
    let log_file = std::fs::File::create(&log).map_err(|e| format!("sidecar log: {e}"))?;
    let mut cmd = Command::new(&env.python);
    cmd.args(["-m", "doc_parser"])
        .current_dir(env.dir.join("app"))
        .env_clear()
        .envs(child_env(env, &scratch, &token_file, port))
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            log_file.try_clone().map_err(|e| e.to_string())?,
        ))
        .stderr(Stdio::from(log_file));
    let child = cmd.spawn().map_err(|e| format!("sidecar spawn: {e}"))?;
    let mut sc = Sidecar {
        child,
        url: format!("http://127.0.0.1:{port}/"),
        token_file,
    };
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_millis(500),
        )
        .is_ok()
        {
            return Ok(sc);
        }
        if let Ok(Some(status)) = sc.child.try_wait() {
            let tail = std::fs::read_to_string(&log).unwrap_or_default();
            let tail: String = tail
                .chars()
                .rev()
                .take(500)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            return Err(format!("the parser sidecar exited ({status}): {tail}"));
        }
        if Instant::now() > deadline {
            return Err("the parser sidecar did not listen within 180 s".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_names_what_is_missing() {
        let d = std::env::temp_dir().join(format!("oo-sc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let e = probe(&d).unwrap_err();
        assert!(
            e.contains("venv/bin/python") && e.contains("unsupported"),
            "{e}"
        );
        std::fs::create_dir_all(d.join("venv/bin")).unwrap();
        std::fs::write(d.join("venv/bin/python"), "").unwrap();
        assert!(probe(&d).unwrap_err().contains("app/doc_parser"));
        std::fs::create_dir_all(d.join("app/doc_parser")).unwrap();
        assert!(probe(&d).unwrap_err().contains("models/"));
        std::fs::create_dir_all(d.join("models")).unwrap();
        assert_eq!(probe(&d).unwrap().python, d.join("venv/bin/python"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn child_env_is_closed_and_offline() {
        let env = SidecarEnv {
            dir: "/s".into(),
            python: "/s/venv/bin/python".into(),
        };
        let v = child_env(&env, Path::new("/scr"), Path::new("/t"), 9);
        let get = |k: &str| v.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone());
        assert_eq!(get("HF_HUB_OFFLINE").as_deref(), Some("1"));
        assert_eq!(get("OO_PARSER_HOST").as_deref(), Some("127.0.0.1"));
        assert_eq!(get("DOCLING_ARTIFACTS_PATH").as_deref(), Some("/s/models"));
        assert!(get("PATH").unwrap().starts_with("/s/venv/bin:"));
        assert!(get("OO_DOCS_DATABASE_URL").is_none());
    }
}
