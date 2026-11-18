//! The Docker-free runtime of `oo serve` (D83, ADR-S14-docker-free-runtime,
//! step 1): a private PostgreSQL cluster in the state directory, supervised by
//! `oo serve`, plus the engine as a host process. It serves Markdown, text and
//! CSV Drives with no container; Office and PDF Drives need the compose runtime
//! or a parser sidecar the operator runs (`OO_DOCS_PARSER_URL`).
//!
//! The cluster lives in `<state>/pg/data`, listens on loopback only (no unix
//! socket) and uses SCRAM logins whose passwords are 0600 files in `<state>/pg`.
//! The server binaries (`initdb`, `pg_ctl`, `psql`) are found through
//! `OO_DOCS_PG_BIN_DIR`, `PATH` and the usual Homebrew and distribution
//! directories; none is bundled yet (signing of a bundled build is open in the
//! ADR). Passwords reach a child only through its environment or stdin, never
//! its argument list.

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::secrets;

const DB: &str = "oo_docs";
const LOGIN: &str = "oo_docs_login";

/// The PostgreSQL server programs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgBins {
    pub dir: PathBuf,
}

impl PgBins {
    fn bin(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// A server program with the locale fixed: on macOS the postmaster refuses
    /// to start ("became multithreaded") when no valid locale is set.
    fn cmd(&self, name: &str) -> Command {
        let mut c = Command::new(self.bin(name));
        c.env("LC_ALL", "C").env("LANG", "C");
        c
    }
}

fn has_all(dir: &Path) -> bool {
    ["initdb", "pg_ctl", "psql"]
        .iter()
        .all(|n| dir.join(n).is_file())
}

/// Find a directory holding `initdb`, `pg_ctl` and `psql`.
pub fn find_bins() -> Option<PgBins> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("OO_DOCS_PG_BIN_DIR") {
        let d = PathBuf::from(d);
        return has_all(&d).then_some(PgBins { dir: d });
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    for root in ["/opt/homebrew/opt", "/usr/local/opt"] {
        for v in [
            "postgresql@17",
            "postgresql@16",
            "postgresql@15",
            "postgresql",
        ] {
            dirs.push(Path::new(root).join(v).join("bin"));
        }
    }
    for v in [17, 16, 15] {
        dirs.push(PathBuf::from(format!("/usr/lib/postgresql/{v}/bin")));
    }
    dirs.into_iter()
        .find(|d| has_all(d))
        .map(|dir| PgBins { dir })
}

/// A running private cluster.
pub struct Embedded {
    bins: PgBins,
    data: PathBuf,
    pub port: u16,
    admin_password: String,
    runtime_password: String,
    stopped: bool,
}

fn free_port() -> Option<u16> {
    let l = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    l.local_addr().ok().map(|a| a.port())
}

fn read_secret(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn secret_or_new(path: &Path) -> Result<String, String> {
    if let Some(s) = read_secret(path) {
        return Ok(s);
    }
    let v = secrets::new_token("").map_err(|_| "no randomness source".to_owned())?;
    secrets::write_secret_file(path, &v).map_err(|_| "cannot write a password file".to_owned())?;
    Ok(v)
}

fn run_ok(cmd: &mut Command, what: &str) -> Result<std::process::Output, String> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{what}: {}", e.kind()))?;
    if out.status.success() {
        Ok(out)
    } else {
        let tail = String::from_utf8_lossy(&out.stderr)
            .lines()
            .last()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>();
        Err(format!("{what} failed: {tail}"))
    }
}

impl Embedded {
    fn running(bins: &PgBins, data: &Path) -> bool {
        bins.cmd("pg_ctl")
            .arg("-D")
            .arg(data)
            .arg("status")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Create the cluster on first use and start it.
    pub fn start(state: &Path, bins: PgBins) -> Result<Self, String> {
        let dir = state.join("pg");
        secrets::ensure_state_dir(&dir).map_err(|e| e.0.to_owned())?;
        let data = dir.join("data");
        let admin_password = secret_or_new(&dir.join("admin.pw"))?;
        let runtime_password = secret_or_new(&dir.join("runtime.pw"))?;
        if !data.join("PG_VERSION").is_file() {
            run_ok(
                bins.cmd("initdb")
                    .arg("-D")
                    .arg(&data)
                    .args([
                        "-U",
                        "postgres",
                        "--auth=scram-sha-256",
                        "-E",
                        "UTF8",
                        "--no-locale",
                    ])
                    .arg(format!("--pwfile={}", dir.join("admin.pw").display())),
                "initdb",
            )?;
        }
        let port_file = dir.join("port");
        let reuse = Self::running(&bins, &data)
            .then(|| read_secret(&port_file).and_then(|p| p.parse::<u16>().ok()))
            .flatten();
        let port = match reuse {
            Some(p) => p,
            None => {
                let port = free_port().ok_or("no free loopback port")?;
                run_ok(
                    bins.cmd("pg_ctl")
                        .arg("-D")
                        .arg(&data)
                        .args(["-w", "-t", "90", "-l"])
                        .arg(dir.join("pg.log"))
                        .arg("-o")
                        .arg(format!(
                            "-p {port} -c listen_addresses=127.0.0.1 -c unix_socket_directories= -c fsync=on"
                        ))
                        .arg("start"),
                    "pg_ctl start",
                )?;
                let _ = std::fs::write(&port_file, port.to_string());
                port
            }
        };
        let me = Self {
            bins,
            data,
            port,
            admin_password,
            runtime_password,
            stopped: false,
        };
        me.sql(
            "postgres",
            &format!(
                "SELECT 'CREATE DATABASE {DB}' WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = '{DB}')\\gexec"
            ),
        )?;
        Ok(me)
    }

    /// Run SQL through `psql` as the admin login; the text goes over stdin.
    fn sql(&self, db: &str, sql: &str) -> Result<(), String> {
        let mut child = self
            .bins
            .cmd("psql")
            .args([
                "-X",
                "-q",
                "-h",
                "127.0.0.1",
                "-U",
                "postgres",
                "-v",
                "ON_ERROR_STOP=1",
                "-d",
                db,
            ])
            .args(["-p", &self.port.to_string()])
            .env("PGPASSWORD", &self.admin_password)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("psql: {}", e.kind()))?;
        if let Some(mut i) = child.stdin.take() {
            let _ = i.write_all(sql.as_bytes());
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("psql: {}", e.kind()))?;
        if out.status.success() {
            Ok(())
        } else {
            let tail = String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("")
                .chars()
                .take(160)
                .collect::<String>();
            Err(format!("psql failed: {tail}"))
        }
    }

    /// `OO_DOCS_DATABASE_URL` and `OO_DOCS_ADMIN_DATABASE_URL` for the schema install.
    pub fn admin_url(&self) -> String {
        format!(
            "postgres://postgres:{}@127.0.0.1:{}/{DB}",
            self.admin_password, self.port
        )
    }

    /// The runtime login of the long-running engine. Call after the schema is
    /// installed (the `oo_docs_runtime` group role comes from the installer).
    pub fn ensure_runtime_login(&self) -> Result<String, String> {
        let pw = &self.runtime_password;
        self.sql(
            DB,
            &format!(
                "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{LOGIN}') THEN \
                 CREATE ROLE {LOGIN} LOGIN PASSWORD '{pw}' IN ROLE oo_docs_runtime; \
                 ELSE ALTER ROLE {LOGIN} LOGIN PASSWORD '{pw}'; END IF; END $$;"
            ),
        )?;
        Ok(format!(
            "postgres://{LOGIN}:{pw}@127.0.0.1:{}/{DB}",
            self.port
        ))
    }

    /// Stop the cluster (fast shutdown). Idempotent.
    pub fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let _ = self
            .bins
            .cmd("pg_ctl")
            .arg("-D")
            .arg(&self.data)
            .args(["-m", "fast", "-w", "-t", "60", "stop"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

impl Drop for Embedded {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bin_dir_needs_all_three_programs() {
        let d = std::env::temp_dir().join(format!("oo-pgbins-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(!has_all(&d));
        for n in ["initdb", "pg_ctl"] {
            std::fs::write(d.join(n), "").unwrap();
        }
        assert!(!has_all(&d));
        std::fs::write(d.join("psql"), "").unwrap();
        assert!(has_all(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_free_port_is_nonzero() {
        assert!(free_port().is_some_and(|p| p > 0));
    }
}
