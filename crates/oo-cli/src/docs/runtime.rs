//! `oo docs runtime install|status`: the Docker-free document parser environment
//! (D83). `oo` builds it itself under `<state>/sidecar` (a durable place, never
//! `/tmp`): a uv-managed CPython 3.13 venv with the pinned packages, the parser
//! package and its models lock (both embedded in this binary), and the model
//! files. Models come from the locked parser image when Docker holds it, else
//! they are downloaded by the pinned sources of `models.lock.json` and verified
//! against its sha256 list. Only our own sidecar dependencies are downloaded.
//! `oo serve` (embedded runtime) installs the environment on first use and then
//! starts it (see `serve.rs`); `OO_DOCS_RUNTIME_AUTOINSTALL=0` turns that off.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

use super::client::exit;
use super::docs_cmd::{out, parse};
use super::{secrets, sidecar};

const REQUIREMENTS: &str = include_str!("../../../../tools/doc-parser/requirements.lock");
const MODELS_LOCK: &str = include_str!("../../../../tools/doc-parser/models.lock.json");
const FETCH_MODELS: &str = include_str!("../../../../tools/doc-parser/fetch_models.py");
/// The PyPI OpenCV wheel bundles FFmpeg: a development stand-in for the image's
/// FFmpeg-free source build (D65), used by the Docker-free sidecar only.
const DEV_OPENCV: &str = "opencv-python-headless==5.0.0.93";
const PARSER_IMAGE: &str = "oo-docs-parser:dev";

macro_rules! parser_files {
    ($($f:literal),* $(,)?) => {
        &[$(($f, include_str!(concat!("../../../../tools/doc-parser/doc_parser/", $f)))),*]
    };
}
const PARSER_PACKAGE: &[(&str, &str)] = parser_files![
    "__init__.py",
    "__main__.py",
    "adapter.py",
    "capabilities.py",
    "config.py",
    "embed.py",
    "meta.py",
    "ooxml.py",
    "pictures.py",
    "scratch.py",
    "server.py",
    "supervisor.py",
    "watchdog.py",
];

pub const MARKER: &str = ".oo-runtime.json";

/// `name==version` pins from the hashed lock, local version suffixes dropped.
pub fn lock_pins(text: &str) -> Vec<String> {
    let mut pins = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        let name_end = line
            .find(|c: char| !(c.is_ascii_alphanumeric() || "_.-[]".contains(c)))
            .unwrap_or(0);
        if name_end == 0 || !line[name_end..].starts_with("==") {
            continue;
        }
        let rest = &line[name_end + 2..];
        let ver_end = rest
            .find(|c: char| c.is_whitespace() || c == ';' || c == '\\')
            .unwrap_or(rest.len());
        let ver = rest[..ver_end].split('+').next().unwrap_or("");
        if !ver.is_empty() {
            pins.push(format!("{}=={ver}", &line[..name_end]));
        }
    }
    pins.push(DEV_OPENCV.to_owned());
    pins
}

/// Whether the environment is complete (marker plus the three probe parts).
pub fn installed(dir: &Path) -> bool {
    dir.join(MARKER).is_file() && sidecar::probe(dir).is_ok()
}

fn find_uv() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join("uv")).collect())
        .unwrap_or_default();
    for d in ["/opt/homebrew/bin", "/usr/local/bin"] {
        cands.push(Path::new(d).join("uv"));
    }
    if let Some(h) = std::env::var_os("HOME") {
        for d in [".local/bin", ".cargo/bin"] {
            cands.push(Path::new(&h).join(d).join("uv"));
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

/// uv uses a cache under the oo state unless the operator set one.
fn uv_cmd(uv: &Path, state: &Path) -> Command {
    let mut c = Command::new(uv);
    if std::env::var_os("UV_CACHE_DIR").is_none() {
        c.env("UV_CACHE_DIR", state.join("uv-cache"));
    }
    c.stdin(Stdio::null());
    c
}

fn run(mut c: Command, what: &str) -> Result<(), String> {
    let o = c
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{what}: {e}"))?;
    if o.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&o.stderr);
    let tail: Vec<&str> = err.lines().rev().take(6).collect();
    Err(format!(
        "{what} failed ({}): {}",
        o.status,
        tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
    ))
}

fn docker_has_image() -> bool {
    super::dockercli::docker()
        .args(["image", "inspect", "--format", "{{.Id}}", PARSER_IMAGE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn models_from_image(dest: &Path) -> Result<(), String> {
    let name = format!("oo-runtime-models-{}", std::process::id());
    let mk = super::dockercli::docker()
        .args(["create", "--label", "oo.lane=oo-runtime-install", "--name"])
        .arg(&name)
        .arg(PARSER_IMAGE)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    if !mk.success() {
        return Err("docker create of the parser image failed".into());
    }
    let cp = super::dockercli::docker()
        .arg("cp")
        .arg(format!("{name}:/opt/models"))
        .arg(dest)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = super::dockercli::docker()
        .args(["rm", &name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match cp {
        Ok(s) if s.success() => Ok(()),
        _ => Err("docker cp of /opt/models failed".into()),
    }
}

/// Build the environment in `dir` (idempotent: finished parts are kept).
pub fn install(state: &Path, dir: &Path, say: &dyn Fn(&str)) -> Result<String, String> {
    let uv = find_uv().ok_or(
        "uv is not installed: the Docker-free parser needs it (https://docs.astral.sh/uv/ , `brew install uv`); markdown, text and CSV work without it",
    )?;
    secrets::ensure_state_dir(state).map_err(|e| e.0.to_owned())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let venv = dir.join("venv");
    if !venv.join("bin/python").is_file() {
        say("creating the Python 3.13 environment");
        let mut c = uv_cmd(&uv, state);
        c.args(["venv", "--python", "3.13", "-q"]).arg(&venv);
        run(c, "uv venv")?;
        let pins = dir.join("pins.txt");
        std::fs::write(&pins, lock_pins(REQUIREMENTS).join("\n") + "\n")
            .map_err(|e| e.to_string())?;
        say("installing the pinned parser packages (large, one time)");
        let mut c = uv_cmd(&uv, state);
        c.args(["pip", "install", "-q", "--no-deps", "--python"])
            .arg(venv.join("bin/python"))
            .arg("-r")
            .arg(&pins);
        if let Err(e) = run(c, "uv pip install") {
            let _ = std::fs::remove_dir_all(&venv);
            return Err(e);
        }
    }
    let app = dir.join("app");
    std::fs::create_dir_all(app.join("doc_parser")).map_err(|e| e.to_string())?;
    for (name, text) in PARSER_PACKAGE {
        std::fs::write(app.join("doc_parser").join(name), text).map_err(|e| e.to_string())?;
    }
    std::fs::write(app.join("models.lock.json"), MODELS_LOCK).map_err(|e| e.to_string())?;
    std::fs::write(app.join("fetch_models.py"), FETCH_MODELS).map_err(|e| e.to_string())?;
    let models = dir.join("models");
    let mut source = "existing";
    if !models.is_dir() {
        let part = dir.join("models.partial");
        let _ = std::fs::remove_dir_all(&part);
        if docker_has_image() && models_from_image(&part).is_ok() {
            source = "locked parser image";
        } else {
            let _ = std::fs::remove_dir_all(&part);
            say("downloading the pinned model files");
            let mut c = Command::new(venv.join("bin/python"));
            c.args(["-I"])
                .arg(app.join("fetch_models.py"))
                .arg("--dest")
                .arg(&part)
                .arg("--lock")
                .arg(app.join("models.lock.json"))
                .args(["--mode", "verify"])
                .env("HOME", state)
                .env("HF_HOME", state.join("hf-cache"))
                .env("PATH", "/usr/bin:/bin")
                .stdin(Stdio::null());
            if let Err(e) = run(c, "model download") {
                let _ = std::fs::remove_dir_all(&part);
                return Err(e);
            }
            source = "pinned sources";
        }
        std::fs::rename(&part, &models).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        dir.join(MARKER),
        json!({"protocol": "oo.docs-runtime/v1", "models": source, "python": "3.13"}).to_string(),
    )
    .map_err(|e| e.to_string())?;
    sidecar::probe(dir)?;
    Ok(source.to_owned())
}

pub fn run_cmd(rest: &[String]) -> u8 {
    let p = match parse(rest, &[], &[]) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("oo docs runtime: {e}");
            return exit::ARGS;
        }
    };
    let Some(state) = p.state_dir.clone().or_else(secrets::default_state_dir) else {
        eprintln!("oo docs runtime: no state directory (set HOME or --state-dir)");
        return super::serve::exit::STATE_DIR;
    };
    let dir = sidecar::env_dir(&state);
    match p.pos.first().map(String::as_str) {
        Some("status") => {
            let ok = installed(&dir);
            let uv = find_uv().is_some();
            out(
                &p,
                "oo.docs-runtime/v1",
                json!({"installed": ok, "dir": dir, "uv": uv}),
                format!(
                    "parser runtime {} ({}){}",
                    if ok { "installed" } else { "not installed" },
                    dir.display(),
                    if !ok && !uv { "; uv is missing" } else { "" }
                ),
            );
            0
        }
        Some("install") => {
            let json = p.json;
            let say = |m: &str| {
                if !json {
                    eprintln!("oo docs runtime: {m}");
                }
            };
            match install(&state, &dir, &say) {
                Ok(src) => {
                    out(
                        &p,
                        "oo.docs-runtime/v1",
                        json!({"installed": true, "dir": dir, "models": src}),
                        format!(
                            "parser runtime installed in {} (models: {src})",
                            dir.display()
                        ),
                    );
                    0
                }
                Err(e) => {
                    eprintln!("oo docs runtime: {e} (DOCS_RUNTIME_INSTALL_FAILED)");
                    exit::OTHER
                }
            }
        }
        _ => {
            eprintln!("Usage: oo docs runtime install|status [--json] [--state-dir <dir>]");
            exit::ARGS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_drop_local_versions_and_hashes() {
        let t = "torch==2.15.0+cpu \\\n    --hash=sha256:aa\nfoo[bar]==1.2 ; python_version < '4'\n# c\n";
        let p = lock_pins(t);
        assert_eq!(p[0], "torch==2.15.0");
        assert_eq!(p[1], "foo[bar]==1.2");
        assert_eq!(p.last().unwrap(), DEV_OPENCV);
    }

    #[test]
    fn the_embedded_lock_yields_pins_and_the_package_is_whole() {
        assert!(lock_pins(REQUIREMENTS).len() > 60);
        assert!(PARSER_PACKAGE.iter().any(|(n, _)| *n == "server.py"));
        assert!(MODELS_LOCK.contains("docling-layout-heron"));
    }

    #[test]
    fn not_installed_without_marker() {
        assert!(!installed(Path::new("/nonexistent-oo-runtime")));
    }
}
