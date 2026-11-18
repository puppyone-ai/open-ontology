//! Database sources for `oo source` (D83): `oo source add postgres|mysql|oracle`
//! registers the database in the stack file the existing pipeline reads
//! (`stack.yaml`, `oo.stack/v1`), without touching how that pipeline works. Only
//! the `sources` list changes, through the pipeline's own parser and validator;
//! the model, deploy and gateway parts stay as `oo init` wrote them, so the
//! file must exist first. The password is a `0600` file in the oo secrets
//! directory and the stack file carries only `secret_ref: file:<path>`.

use std::io::IsTerminal;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use oo_stack::stack::{self, SecretRef, Snapshot, Source, Stack};
use serde_json::{Value, json};

use super::client::{CliError, exit};
use super::docs_cmd::{Parsed, bad, out};
use super::source::{
    env_value, own_secret_file_of, prompt_hidden, read_stdin_text, slug, store_secret,
};

pub const KINDS: &[&str] = &["postgres", "mysql", "oracle"];

pub fn is_db_kind(kind: &str) -> bool {
    KINDS.contains(&kind)
}

fn default_port(kind: &str) -> u16 {
    match kind {
        "mysql" => 3306,
        "oracle" => 1521,
        _ => 5432,
    }
}

fn err(code: &str, msg: impl Into<String>) -> CliError {
    CliError::new(exit::ARGS, code, msg)
}

/// The stack file named by `--config` (default `stack.yaml` in the directory).
pub fn config_path(p: &Parsed) -> PathBuf {
    PathBuf::from(p.opt("--config").unwrap_or("stack.yaml"))
}

/// Read and validate the stack file. `Ok(None)` when it does not exist.
pub fn load(path: &Path) -> Result<Option<Stack>, CliError> {
    match std::fs::symlink_metadata(path) {
        Err(_) => return Ok(None),
        Ok(m) if !m.is_file() => {
            return Err(err(
                "SO_SOURCE_BAD_STACK",
                format!("{} is not a regular file", path.display()),
            ));
        }
        Ok(_) => {}
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| err("SO_SOURCE_BAD_STACK", format!("{}: {e}", path.display())))?;
    stack::parse(&text).map(Some).map_err(|c| {
        err(
            "SO_SOURCE_BAD_STACK",
            format!("{} is not a valid stack file ({c})", path.display()),
        )
    })
}

/// Write the stack file as JSON, atomically, mode 0600 (the pipeline's own `init --config` normal form).
fn save(path: &Path, s: &Stack) -> Result<(), CliError> {
    stack::validate(s).map_err(|c| {
        err(
            "SO_SOURCE_BAD_STACK",
            format!("the result is not a valid stack ({c})"),
        )
    })?;
    let text = serde_json::to_string_pretty(s)
        .map_err(|_| err("SO_SOURCE_BAD_STACK", "cannot serialize"))?;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| {
                CliError::new(
                    exit::OTHER,
                    "SO_SOURCE_BAD_STACK",
                    format!("{}: {e}", tmp.display()),
                )
            })?;
        f.write_all(text.as_bytes())
            .and_then(|()| f.write_all(b"\n"))
            .and_then(|()| f.sync_all())
            .map_err(|e| CliError::new(exit::OTHER, "SO_SOURCE_BAD_STACK", e.to_string()))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        CliError::new(
            exit::OTHER,
            "SO_SOURCE_BAD_STACK",
            format!("{}: {e}", path.display()),
        )
    })
}

fn file_ref(path: &str, what: &str) -> Result<String, CliError> {
    let abs =
        std::fs::canonicalize(path).map_err(|_| bad(format!("{what} {path} does not exist")))?;
    let r = format!("file:{}", abs.display());
    if stack::secret_reference(&r) {
        Ok(r)
    } else {
        Err(bad(format!("{what} is not a usable path")))
    }
}

/// Where the password comes from.
enum Pw {
    Reference(String),
    Stdin,
    Env(String),
    Prompt,
}

fn password_source(p: &Parsed, interactive: bool) -> Result<Pw, CliError> {
    let given = [
        p.opt("--password-ref").is_some(),
        p.has("--password-stdin"),
        p.opt("--password-env").is_some(),
        p.opt("--password-file").is_some(),
    ]
    .iter()
    .filter(|b| **b)
    .count();
    if given > 1 {
        return Err(bad(
            "choose one: --password-stdin, --password-env, --password-file or --password-ref",
        ));
    }
    if let Some(r) = p.opt("--password-ref") {
        if !stack::secret_reference(r) {
            return Err(bad(
                "--password-ref is env:NAME, file:/absolute/path or a secret name",
            ));
        }
        return Ok(Pw::Reference(r.to_owned()));
    }
    if let Some(f) = p.opt("--password-file") {
        return Ok(Pw::Reference(file_ref(f, "--password-file")?));
    }
    if p.has("--password-stdin") {
        return Ok(Pw::Stdin);
    }
    if let Some(v) = p.opt("--password-env") {
        return Ok(Pw::Env(v.to_owned()));
    }
    if interactive {
        return Ok(Pw::Prompt);
    }
    Err(bad(
        "no password given and no terminal to ask: use --password-stdin, --password-env, --password-file or --password-ref",
    ))
}

/// `(secret_ref, created file)`.
fn acquire_password(src: Pw, file_name: &str) -> Result<(String, Option<PathBuf>), CliError> {
    let text = match src {
        Pw::Reference(r) => return Ok((r, None)),
        Pw::Stdin => read_stdin_text()?.trim_end_matches(['\n', '\r']).to_owned(),
        Pw::Env(v) => env_value(&v)?,
        Pw::Prompt => prompt_hidden("Database password").unwrap_or_default(),
    };
    if text.is_empty() {
        return Err(err("SO_SOURCE_BAD_CREDENTIAL", "the password was empty"));
    }
    let (reference, path) = store_secret(file_name, &text)?;
    Ok((reference, Some(path)))
}

/// Build the stack source from the flags (the password reference is given).
pub fn build_source(
    p: &Parsed,
    kind: &str,
    name: &str,
    password_ref: String,
) -> Result<Source, CliError> {
    let host = p.opt("--host").ok_or_else(|| bad("--host is required"))?;
    let database = p
        .opt("--database")
        .ok_or_else(|| bad("--database is required (the service name for oracle)"))?;
    let user = p.opt("--user").ok_or_else(|| bad("--user is required"))?;
    let port = match p.opt("--port") {
        Some(v) => v.parse::<u16>().map_err(|_| bad("--port is a number"))?,
        None => default_port(kind),
    };
    let tls_ca = match (p.opt("--tls-ca-file"), p.opt("--tls-ca-ref")) {
        (Some(_), Some(_)) => return Err(bad("choose --tls-ca-file or --tls-ca-ref")),
        (Some(f), None) => Some(file_ref(f, "--tls-ca-file")?),
        (None, Some(r)) if stack::secret_reference(r) => Some(r.to_owned()),
        (None, Some(_)) => {
            return Err(bad(
                "--tls-ca-ref is env:NAME, file:/absolute/path or a secret name",
            ));
        }
        (None, None) => None,
    };
    let include: Vec<String> = p.all("--include").iter().map(|s| (*s).to_owned()).collect();
    if matches!(kind, "postgres" | "mysql") {
        if tls_ca.is_none() {
            return Err(bad(
                "postgres and mysql sources need a trust anchor: --tls-ca-file <pem> or --tls-ca-ref",
            ));
        }
        if include.is_empty() {
            return Err(bad(
                "postgres and mysql sources need an explicit scope: --include <schema.table> (repeatable)",
            ));
        }
    }
    let mut scope = stack::Scope::default();
    if !include.is_empty() {
        scope.include = include;
    }
    scope.exclude = p.all("--exclude").iter().map(|s| (*s).to_owned()).collect();
    Ok(Source {
        id: name.to_owned(),
        engine: kind.to_owned(),
        host: host.to_owned(),
        port,
        database: database.to_owned(),
        user: user.to_owned(),
        password: SecretRef {
            secret_ref: password_ref,
        },
        tls_ca: tls_ca.map(|secret_ref| SecretRef { secret_ref }),
        ssh: None,
        scope,
        sync: stack::Sync::default(),
        snapshot: Snapshot::default(),
    })
}

/// What may be shown of one stack source.
pub fn public(s: &Source) -> Value {
    json!({
        "name": s.id,
        "backend": s.engine,
        "kind": "database",
        "status": "configured",
        "files_total": Value::Null,
        "config": {
            "host": s.host, "port": s.port, "database": s.database, "user": s.user,
            "secret_ref": "(set)",
            "tls_ca": s.tls_ca.is_some(),
            "include": s.scope.include, "exclude": s.scope.exclude,
        }
    })
}

/// The database sources of the stack file at `--config` (none when it is absent).
pub fn list(p: &Parsed) -> Result<Vec<Value>, CliError> {
    Ok(load(&config_path(p))?
        .map(|s| s.sources.iter().map(public).collect())
        .unwrap_or_default())
}

/// Is `name` a source of the stack file?
pub fn exists(p: &Parsed, name: &str) -> bool {
    load(&config_path(p))
        .ok()
        .flatten()
        .is_some_and(|s| s.sources.iter().any(|x| x.id == name))
}

pub fn add(p: &Parsed, kind: &str) -> Result<(), CliError> {
    let name = p.opt("--name").ok_or_else(|| bad("--name is required"))?;
    if !stack::secret_name(name) {
        return Err(bad("--name is letters, digits, - and _ (at most 64)"));
    }
    let path = config_path(p);
    let mut s = load(&path)?.ok_or_else(|| {
        err(
            "SO_SOURCE_NO_STACK",
            format!("{} does not exist; `oo init` creates it (the model the database pipeline needs is set there), or pass --config <file>", path.display()),
        )
    })?;
    if s.sources.iter().any(|x| x.id == name) {
        return Err(err(
            "SO_SOURCE_EXISTS",
            format!("a source named `{name}` is already in {}", path.display()),
        ));
    }
    // build first with a placeholder to fail on bad flags before any secret is written
    build_source(p, kind, name, "env:OO_PLACEHOLDER".into())?;
    let interactive = std::io::stdin().is_terminal() && !p.has("--non-interactive");
    let src = password_source(p, interactive)?;
    let (reference, created) = acquire_password(src, &format!("src_{}_pw", slug(name)))?;
    let result = build_source(p, kind, name, reference).and_then(|src| {
        let shown = public(&src);
        s.sources.push(src);
        save(&path, &s).map(|()| shown)
    });
    match result {
        Ok(mut shown) => {
            shown["credential_file_created"] = json!(created.is_some());
            shown["stack_file"] = json!(path.display().to_string());
            let human = format!(
                "added {kind} source {name} to {} (run `oo doctor` to probe it)",
                path.display()
            );
            out(p, "oo.source/v1", shown, human);
            Ok(())
        }
        Err(e) => {
            if let Some(f) = created {
                let _ = std::fs::remove_file(f);
            }
            Err(e)
        }
    }
}

pub fn show(p: &Parsed, name: &str) -> Result<(), CliError> {
    let s = load(&config_path(p))?.ok_or_else(|| bad("no stack file"))?;
    let src = s
        .sources
        .iter()
        .find(|x| x.id == name)
        .ok_or_else(|| bad(format!("no source {name}")))?;
    let v = public(src);
    let human = format!(
        "{} {} {}:{}/{}",
        src.id, src.engine, src.host, src.port, src.database
    );
    out(p, "oo.source/v1", v, human);
    Ok(())
}

pub fn rm(p: &Parsed, name: &str) -> Result<(), CliError> {
    let path = config_path(p);
    let mut s = load(&path)?.ok_or_else(|| bad("no stack file"))?;
    let idx = s
        .sources
        .iter()
        .position(|x| x.id == name)
        .ok_or_else(|| bad(format!("no source {name}")))?;
    if s.sources.len() == 1 {
        return Err(err(
            "SO_SOURCE_LAST",
            "a stack needs at least one database source; edit the file or run `oo init` instead",
        ));
    }
    let removed = s.sources.remove(idx);
    save(&path, &s)?;
    let mut gone = false;
    if !p.has("--keep-secret")
        && let Some(f) = own_secret_file_of(&removed.password.secret_ref)
    {
        gone = std::fs::remove_file(f).is_ok();
    }
    out(
        p,
        "oo.source/v1",
        json!({"name": name, "removed": true, "credential_file_removed": gone}),
        format!(
            "removed {name}{}",
            if gone {
                "; its credential file was deleted"
            } else {
                ""
            }
        ),
    );
    Ok(())
}

/// A cheap reachability check; the full read-only probe is `oo doctor`.
pub fn test(p: &Parsed, name: &str) -> Result<(), CliError> {
    use std::net::ToSocketAddrs;
    let s = load(&config_path(p))?.ok_or_else(|| bad("no stack file"))?;
    let src = s
        .sources
        .iter()
        .find(|x| x.id == name)
        .ok_or_else(|| bad(format!("no source {name}")))?;
    let reach = (src.host.as_str(), src.port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .is_some_and(|a| {
            std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(5)).is_ok()
        });
    let report = json!({"name": name, "backend": src.engine, "reachable": reach, "ok": reach,
        "note": "TCP reachability only; `oo doctor` signs in and reads the catalog"});
    out(
        p,
        "oo.source-test/v1",
        report,
        format!(
            "{} {name}: {}",
            if reach { "ok" } else { "FAILED" },
            if reach { "reachable" } else { "unreachable" }
        ),
    );
    if reach {
        Ok(())
    } else {
        Err(CliError::new(
            exit::OTHER,
            "SO_SOURCE_TEST_FAILED",
            "the database host did not accept a connection",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Parsed {
        let a: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        super::super::docs_cmd::parse(
            &a,
            super::super::source::VALUED,
            super::super::source::SWITCHES,
        )
        .unwrap()
    }

    #[test]
    fn postgres_needs_trust_anchor_and_scope() {
        let base = [
            "--name",
            "erp",
            "--host",
            "db.local",
            "--database",
            "app",
            "--user",
            "ro",
        ];
        let e = build_source(&parsed(&base), "postgres", "erp", "x".into())
            .err()
            .unwrap();
        assert!(e.message.contains("trust anchor"), "{}", e.message);
        let mut a = base.to_vec();
        a.extend(["--tls-ca-ref", "env:CA"]);
        let e = build_source(&parsed(&a), "postgres", "erp", "x".into())
            .err()
            .unwrap();
        assert!(e.message.contains("explicit scope"), "{}", e.message);
        a.extend(["--include", "public.orders"]);
        let s = build_source(&parsed(&a), "postgres", "erp", "file:/s/p".into())
            .ok()
            .unwrap();
        assert_eq!((s.port, s.engine.as_str()), (5432, "postgres"));
        assert_eq!(s.scope.include, vec!["public.orders"]);
    }

    #[test]
    fn oracle_defaults_and_public_hides_the_reference() {
        let a = [
            "--name",
            "ora",
            "--host",
            "h",
            "--database",
            "SVC",
            "--user",
            "u",
        ];
        let s = build_source(&parsed(&a), "oracle", "ora", "file:/secret/path".into())
            .ok()
            .unwrap();
        assert_eq!(s.port, 1521);
        let v = public(&s);
        assert_eq!(v["config"]["secret_ref"], "(set)");
        assert!(!v.to_string().contains("/secret/path"));
    }
}
