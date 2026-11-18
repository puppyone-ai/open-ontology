//! The `docker` command as the user's Docker CLI sees it. The CLI keeps its
//! contexts and plugins (Compose among them) in `$DOCKER_CONFIG`, default
//! `$HOME/.docker`. When `HOME` points somewhere else (a sandboxed or test
//! home) that directory is empty and `docker compose` disappears; then the
//! account's own `~/.docker` from the password database is used.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The Docker config directory to force, if the current environment hides the real one.
pub fn fallback_config() -> Option<&'static Path> {
    static CELL: OnceLock<Option<PathBuf>> = OnceLock::new();
    CELL.get_or_init(|| {
        if std::env::var_os("DOCKER_CONFIG").is_some() {
            return None;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if home.as_ref().is_some_and(|h| {
            h.join(".docker/config.json").exists() || h.join(".docker/cli-plugins").exists()
        }) {
            return None;
        }
        let real = account_home()?;
        (home.as_deref() != Some(real.as_path()) && real.join(".docker").is_dir())
            .then(|| real.join(".docker"))
    })
    .as_deref()
}

/// The login account's home directory from the password database (`~user`).
fn account_home() -> Option<PathBuf> {
    let out = Command::new("sh")
        .args(["-c", r#"eval echo "~$(id -un)""#])
        .output()
        .ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let p = PathBuf::from(s.trim());
    (out.status.success() && p.is_absolute()).then_some(p)
}

/// `docker` with the right config directory.
pub fn docker() -> Command {
    let mut c = Command::new("docker");
    if let Some(cfg) = fallback_config() {
        c.env("DOCKER_CONFIG", cfg);
    }
    c
}
