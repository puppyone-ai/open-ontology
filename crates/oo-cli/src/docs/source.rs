//! `oo source add|ls|show|rm|test` (D83, WP1): register where the documents
//! live, as a thin client of the control API.
//!
//! ```text
//! oo source add fs --name <n> --root <dir> [--exclude <glob>]...
//! oo source add s3|oss|cos|r2 --name <n> --bucket <b> [--prefix <p>] [--endpoint <url>] [--region <r>]
//!        [--path-style] [--account-id <id>] [--internal] [--exclude <glob>]...
//!        [--secret-ref <ref> | --credentials-stdin | --access-key-id-env <A> --secret-access-key-env <S> [--session-token-env <T>]]
//! oo source ls | show <name> | rm <name> [--keep-secret] | test <name>
//! ```
//!
//! Credentials come from a hidden prompt (terminal only), stdin, environment
//! variables named on the command line, or an existing reference. They are never
//! accepted as a command-line value. The CLI stores a new credential as a
//! `0600` file in the oo secrets directory (`O_EXCL`, no symlink) and registers
//! the Drive with `secret_ref: file:<absolute path>`; no key or secret ever
//! reaches the engine configuration, a log or a JSON answer. The object-store
//! pipeline and the database pipeline stay separate: this command only creates
//! Drives.

use std::io::{IsTerminal, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::client::{CliError, Daemon, exit};
use super::docs_cmd::{Parsed, bad, drive_id, out, parse, rev_header, state_of};
use super::upload::{host_root, read_drive_secret, s3_target};

pub const HELP: &str = "Usage:\n  oo source add fs --name <n> --root <dir> [--exclude <glob>]...\n  oo source add s3|oss|cos|r2 --name <n> --bucket <b> [--prefix <p>] [--endpoint <url>] [--region <r>] [--path-style] [--account-id <id>] [--internal] [--exclude <glob>]...\n      credentials (one of): a hidden prompt on a terminal | --credentials-stdin (JSON or two lines: key id, secret) | --access-key-id-env <VAR> --secret-access-key-env <VAR> [--session-token-env <VAR>] | --secret-ref env:VAR|file:/abs|NAME\n  oo source ls | show <name> | rm <name> [--keep-secret] | test <name>\n  (all take --json and --state-dir <dir>; none prompts without a terminal)\n";

const VALUED: &[&str] = &[
    "--name",
    "--root",
    "--bucket",
    "--prefix",
    "--endpoint",
    "--region",
    "--account-id",
    "--exclude",
    "--secret-ref",
    "--access-key-id-env",
    "--secret-access-key-env",
    "--session-token-env",
];
const SWITCHES: &[&str] = &[
    "--path-style",
    "--no-path-style",
    "--internal",
    "--credentials-stdin",
    "--keep-secret",
    "--model",
    "--yes",
    "--non-interactive",
];

/// Where the secret files live: `OO_DOCS_SECRETS_DIR` (the engine reads the same
/// variable) or `$HOME/.config/open-ontology/secrets`.
pub fn secrets_dir() -> Result<PathBuf, CliError> {
    if let Some(d) = std::env::var_os("OO_DOCS_SECRETS_DIR") {
        return Ok(PathBuf::from(d));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| CliError::new(exit::ARGS, "SO_SOURCE_NO_HOME", "HOME is not set"))?;
    Ok(Path::new(&home).join(".config/open-ontology/secrets"))
}

/// Open the secrets directory, creating it (0700) on first use.
pub fn open_store() -> Result<(oo_stack::secret::SecretStore, PathBuf), CliError> {
    let dir = secrets_dir()?;
    if let Some(parent) = dir.parent() {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(parent)
            .map_err(|_| {
                CliError::new(
                    exit::OTHER,
                    "SO_SOURCE_SECRET_STORE",
                    "cannot create the secrets parent directory",
                )
            })?;
    }
    let store = oo_stack::secret::SecretStore::create(&dir).map_err(|c| {
        CliError::new(
            exit::OTHER,
            "SO_SOURCE_SECRET_STORE",
            format!("the secrets directory is not usable ({c})"),
        )
    })?;
    // The store canonicalises its root; report the real path.
    let real = std::fs::canonicalize(&dir).unwrap_or(dir);
    Ok((store, real))
}

/// Characters a secret file name keeps.
pub fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    s.trim_matches('-').chars().take(40).collect()
}

/// Read one line without echo (terminal) from stdin; `None` when stdin is not a
/// terminal. Echo is switched off with `stty` (the crate forbids `unsafe`) and
/// the saved settings are restored afterwards.
pub fn prompt_hidden(label: &str) -> Option<String> {
    use std::process::{Command, Stdio};
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return None;
    }
    let saved = Command::new("stty")
        .arg("-g")
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())?;
    eprint!("{label}: ");
    let _ = std::io::stderr().flush();
    let quiet = Command::new("stty")
        .arg("-echo")
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !quiet {
        eprintln!();
        return None;
    }
    let mut line = String::new();
    let ok = stdin.read_line(&mut line).is_ok();
    let _ = Command::new("stty")
        .arg(&saved)
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .status();
    eprintln!();
    ok.then(|| line.trim_end_matches(['\n', '\r']).to_owned())
}

/// The credential JSON from `--credentials-stdin` text: a JSON object, or
/// lines `access key id`, `secret access key` and optionally `session token`.
pub fn credentials_from_text(text: &str) -> Result<String, CliError> {
    let t = text.trim();
    let invalid = |m: &str| CliError::new(exit::ARGS, "SO_SOURCE_BAD_CREDENTIAL", m.to_owned());
    if t.starts_with('{') {
        let (id, secret, token) = super::s3io::parse_credentials(t).map_err(|m| invalid(&m))?;
        return Ok(credential_json(&id, &secret, token.as_deref()));
    }
    let mut lines = t.lines().map(str::trim).filter(|l| !l.is_empty());
    let id = lines
        .next()
        .ok_or_else(|| invalid("stdin needs the access key id on the first line"))?;
    let secret = lines
        .next()
        .ok_or_else(|| invalid("stdin needs the secret access key on the second line"))?;
    Ok(credential_json(id, secret, lines.next()))
}

pub fn credential_json(id: &str, secret: &str, token: Option<&str>) -> String {
    let mut v = json!({"access_key_id": id, "secret_access_key": secret});
    if let Some(t) = token {
        v["session_token"] = json!(t);
    }
    v.to_string()
}

/// Where a credential for a new source comes from.
#[derive(Debug, PartialEq, Eq)]
pub enum CredentialSource {
    Reference(String),
    Stdin,
    Env {
        id: String,
        secret: String,
        token: Option<String>,
    },
    Prompt,
}

pub fn credential_source(p: &Parsed, interactive: bool) -> Result<CredentialSource, CliError> {
    let reference = p.opt("--secret-ref");
    let id_env = p.opt("--access-key-id-env");
    let secret_env = p.opt("--secret-access-key-env");
    let stdin = p.has("--credentials-stdin");
    let given = usize::from(reference.is_some())
        + usize::from(stdin)
        + usize::from(id_env.is_some() || secret_env.is_some());
    if given > 1 {
        return Err(bad(
            "choose one credential source: --secret-ref, --credentials-stdin or the --*-env pair",
        ));
    }
    if let Some(r) = reference {
        if !oo_stack::stack::secret_reference(r) {
            return Err(bad(
                "--secret-ref is env:NAME, file:/absolute/path or a secret name",
            ));
        }
        return Ok(CredentialSource::Reference(r.to_owned()));
    }
    if stdin {
        return Ok(CredentialSource::Stdin);
    }
    if id_env.is_some() || secret_env.is_some() {
        let (Some(i), Some(s)) = (id_env, secret_env) else {
            return Err(bad(
                "--access-key-id-env and --secret-access-key-env go together",
            ));
        };
        return Ok(CredentialSource::Env {
            id: i.to_owned(),
            secret: s.to_owned(),
            token: p.opt("--session-token-env").map(str::to_owned),
        });
    }
    if interactive {
        return Ok(CredentialSource::Prompt);
    }
    Err(bad(
        "no credential given and no terminal to ask: use --credentials-stdin, --access-key-id-env/--secret-access-key-env, or --secret-ref",
    ))
}

fn read_stdin_text() -> Result<String, CliError> {
    let mut s = String::new();
    std::io::stdin()
        .take(65_537)
        .read_to_string(&mut s)
        .map_err(|_| CliError::new(exit::ARGS, "SO_SOURCE_BAD_CREDENTIAL", "cannot read stdin"))?;
    Ok(s)
}

fn env_value(name: &str) -> Result<String, CliError> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            CliError::new(
                exit::ARGS,
                "SO_SOURCE_BAD_CREDENTIAL",
                format!("environment variable {name} is not set"),
            )
        })
}

/// Create the secret file for `name` and return the `file:` reference.
fn store_secret(name: &str, text: &str) -> Result<(String, PathBuf), CliError> {
    let (store, dir) = open_store()?;
    store.write_new(name, text.as_bytes()).map_err(|c| {
        CliError::new(
            exit::OTHER,
            "SO_SOURCE_SECRET_EXISTS",
            format!("cannot create credential file `{name}` ({c}); remove the old source first or pick another --name"),
        )
    })?;
    let path = dir.join(name);
    Ok((format!("file:{}", path.display()), path))
}

/// Resolve the credential source to a reference, creating a file when needed.
/// Returns `(secret_ref, created_file)`.
fn acquire(src: CredentialSource, file_name: &str) -> Result<(String, Option<PathBuf>), CliError> {
    let text = match src {
        CredentialSource::Reference(r) => return Ok((r, None)),
        CredentialSource::Stdin => credentials_from_text(&read_stdin_text()?)?,
        CredentialSource::Env { id, secret, token } => {
            let tok = token.as_deref().map(env_value).transpose()?;
            credential_json(&env_value(&id)?, &env_value(&secret)?, tok.as_deref())
        }
        CredentialSource::Prompt => {
            let id = prompt_hidden("Access key id").unwrap_or_default();
            let secret = prompt_hidden("Secret access key").unwrap_or_default();
            if id.trim().is_empty() || secret.trim().is_empty() {
                return Err(CliError::new(
                    exit::ARGS,
                    "SO_SOURCE_BAD_CREDENTIAL",
                    "the credential was empty",
                ));
            }
            credential_json(id.trim(), secret.trim(), None)
        }
    };
    let (reference, path) = store_secret(file_name, &text)?;
    Ok((reference, Some(path)))
}

fn location(v: &Value) -> String {
    match v["backend"].as_str().unwrap_or("") {
        "fs" => v["config"]["root"].as_str().unwrap_or("").to_owned(),
        _ => format!(
            "{}/{}",
            v["config"]["bucket"].as_str().unwrap_or(""),
            v["config"]["prefix"].as_str().unwrap_or("")
        ),
    }
}

fn line(v: &Value) -> String {
    format!(
        "{:<16} {:<4} {:<9} files {:<5} {}",
        v["name"].as_str().unwrap_or(""),
        v["backend"].as_str().unwrap_or(""),
        v["status"].as_str().unwrap_or(""),
        v["files_total"],
        location(v)
    )
}

/// What may leave the CLI about a Drive: never the credential reference path in JSON.
fn public(v: &Value) -> Value {
    let mut c = v.clone();
    if let Some(cfg) = c["config"].as_object_mut()
        && cfg.contains_key("secret_ref")
    {
        cfg.insert("secret_ref".into(), json!("(set)"));
    }
    c
}

/// Entry for `oo source <rest...>`; returns the exit code.
pub fn run(rest: &[String]) -> u8 {
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return exit::OK;
    }
    let (Some(sub), tail) = (rest.first(), rest.get(1..).unwrap_or(&[])) else {
        eprint!("{HELP}");
        return exit::ARGS;
    };
    let p = match parse(tail, VALUED, SWITCHES) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("oo source {sub}: {e}");
            return exit::ARGS;
        }
    };
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return exit::OTHER;
    };
    let result = rt.block_on(async {
        let state = state_of(&p)?;
        let d = Daemon::connect(&state)?;
        match sub.as_str() {
            "add" => add(&d, &p).await,
            "ls" => ls(&d, &p).await,
            "show" => show(&d, &p).await,
            "rm" => rm(&d, &p).await,
            "test" => test(&d, &p, &state).await,
            other => Err(bad(format!("unknown command `source {other}`"))),
        }
    });
    match result {
        Ok(()) => exit::OK,
        Err(e) => {
            if p.json {
                eprintln!(
                    "{}",
                    json!({"protocol": "oo.docs-error/v1", "error": {"code": e.code, "message": e.message}})
                );
            } else {
                eprintln!("oo source {sub}: {} ({})", e.message, e.code);
            }
            e.exit
        }
    }
}

async fn add(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let kind = p
        .pos
        .first()
        .map(String::as_str)
        .ok_or_else(|| bad("expected a kind: fs, s3, oss, cos or r2"))?;
    let name = p.opt("--name").ok_or_else(|| bad("--name is required"))?;
    let mut body = json!({"name": name, "backend": kind});
    let mut created: Option<PathBuf> = None;
    match kind {
        "fs" => {
            let root = p.opt("--root").ok_or_else(|| bad("--root is required"))?;
            let root = std::fs::canonicalize(root).map_err(|_| bad("--root does not exist"))?;
            body["config"] = json!({"root": root.display().to_string()});
        }
        "s3" | "oss" | "cos" | "r2" => {
            let bucket = p
                .opt("--bucket")
                .ok_or_else(|| bad("--bucket is required"))?;
            let mut cfg = json!({"bucket": bucket});
            for (flag, key) in [
                ("--prefix", "prefix"),
                ("--endpoint", "endpoint"),
                ("--region", "region"),
                ("--account-id", "account_id"),
            ] {
                if let Some(v) = p.opt(flag) {
                    cfg[key] = json!(v);
                }
            }
            if p.has("--path-style") {
                cfg["path_style"] = json!(true);
            } else if p.has("--no-path-style") {
                cfg["path_style"] = json!(false);
            }
            if p.has("--internal") {
                cfg["internal"] = json!(true);
            }
            let interactive = std::io::stdin().is_terminal() && !p.has("--non-interactive");
            let src = credential_source(p, interactive)?;
            let (reference, file) = acquire(src, &format!("src_{}_s3", slug(name)))?;
            cfg["secret_ref"] = json!(reference);
            created = file;
            body["config"] = cfg;
        }
        other => {
            return Err(bad(format!(
                "unknown source kind `{other}` (fs, s3, oss, cos, r2; databases use `oo init`)"
            )));
        }
    }
    let exclude = p.all("--exclude");
    if !exclude.is_empty() {
        body["config"]["exclude"] = json!(exclude);
    }
    if p.has("--model") {
        body["model_enabled"] = json!(true);
    }
    match d.post_json("/v1/docs/drives", &body).await {
        Ok(v) => {
            let human = format!("added {}", line(&v));
            let mut shown = public(&v);
            shown["credential_file_created"] = json!(created.is_some());
            out(p, "oo.source/v1", shown, human);
            Ok(())
        }
        Err(e) => {
            // The credential was made for this registration only.
            if let Some(f) = created {
                let _ = std::fs::remove_file(f);
            }
            Err(e)
        }
    }
}

async fn ls(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let v = d.get_json("/v1/docs/drives?limit=200").await?;
    let items: Vec<Value> = v["items"].as_array().cloned().unwrap_or_default();
    let human = if items.is_empty() {
        "no sources; `oo docs upload <files>` creates the managed folder, `oo source add` registers more".to_owned()
    } else {
        items.iter().map(line).collect::<Vec<_>>().join("\n")
    };
    let shown: Vec<Value> = items.iter().map(public).collect();
    out(p, "oo.sources/v1", json!({"sources": shown}), human);
    Ok(())
}

async fn show(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let id = drive_id(
        d,
        p.pos
            .first()
            .map(String::as_str)
            .ok_or_else(|| bad("expected <name>"))?,
    )
    .await?;
    let v = d.get_json(&format!("/v1/docs/drives/{id}")).await?;
    out(p, "oo.source/v1", public(&v), line(&v));
    Ok(())
}

/// The secret file a source created for itself, if its reference points into
/// the secrets directory with the `src_` prefix this command writes.
fn own_secret_file(config: &Value) -> Option<PathBuf> {
    let path = PathBuf::from(config["secret_ref"].as_str()?.strip_prefix("file:")?);
    let dir = secrets_dir().ok()?;
    let dir = std::fs::canonicalize(dir).ok()?;
    let name = path.file_name()?.to_str()?;
    (path.parent() == Some(dir.as_path()) && name.starts_with("src_")).then_some(path)
}

async fn rm(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let id = drive_id(
        d,
        p.pos
            .first()
            .map(String::as_str)
            .ok_or_else(|| bad("expected <name>"))?,
    )
    .await?;
    let v = d.get_json(&format!("/v1/docs/drives/{id}")).await?;
    let rev = v["revision"].as_i64().unwrap_or(1);
    let r = d
        .delete(&format!("/v1/docs/drives/{id}"), Some(&rev_header(rev)))
        .await?;
    let mut removed_secret = false;
    if !p.has("--keep-secret")
        && let Some(f) = own_secret_file(&v["config"])
    {
        removed_secret = std::fs::remove_file(f).is_ok();
    }
    let mut shown = public(&r);
    if shown.is_object() {
        shown["credential_file_removed"] = json!(removed_secret);
    }
    out(
        p,
        "oo.source/v1",
        shown,
        format!(
            "removing {}{}",
            v["name"].as_str().unwrap_or(&id),
            if removed_secret {
                "; its credential file was deleted"
            } else {
                ""
            }
        ),
    );
    Ok(())
}

async fn test(d: &Daemon, p: &Parsed, state: &Path) -> Result<(), CliError> {
    let id = drive_id(
        d,
        p.pos
            .first()
            .map(String::as_str)
            .ok_or_else(|| bad("expected <name>"))?,
    )
    .await?;
    let v = d.get_json(&format!("/v1/docs/drives/{id}")).await?;
    let backend = v["backend"].as_str().unwrap_or("fs").to_owned();
    let mut report = json!({"name": v["name"], "backend": backend, "status": v["status"]});
    let mut ok = true;
    if backend == "fs" {
        let root = host_root(state, &v);
        let readable = root.as_ref().is_ok_and(|r| std::fs::read_dir(r).is_ok());
        ok = readable;
        report["folder_readable"] = json!(readable);
        report["writable"] = json!(
            root.as_ref()
                .is_ok_and(|r| std::fs::metadata(r).is_ok_and(|m| m.mode() & 0o200 != 0))
        );
    } else {
        match read_drive_secret(&v["config"]).and_then(|_| s3_target(&v)) {
            Ok(t) => match t.probe_read().await {
                Ok(r) if r.status == 200 => report["read"] = json!(true),
                Ok(r) => {
                    ok = false;
                    report["read"] = json!(false);
                    report["store_status"] = json!(r.status);
                    report["store_code"] = json!(r.code);
                }
                Err(m) => {
                    ok = false;
                    report["read"] = json!(false);
                    report["error"] = json!(m);
                }
            },
            Err(e) => {
                ok = false;
                report["error"] = json!(e.message);
                report["error_code"] = json!(e.code);
            }
        }
        report["writable"] = json!("not probed (a write is only attempted by `oo docs upload`)");
    }
    report["ok"] = json!(ok);
    let human = format!("{} {}", if ok { "ok" } else { "FAILED" }, report);
    out(p, "oo.source-test/v1", report, human);
    if ok {
        Ok(())
    } else {
        Err(CliError::new(
            exit::OTHER,
            "SO_SOURCE_TEST_FAILED",
            "the source did not answer as expected",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Parsed {
        let a: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        parse(&a, VALUED, SWITCHES).unwrap()
    }

    #[test]
    fn credentials_json_and_lines_both_parse() {
        let j =
            credentials_from_text(r#"{"access_key_id":"AK","secret_access_key":"SK"}"#).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&j).unwrap()["secret_access_key"],
            "SK"
        );
        let l = credentials_from_text("AK\nSK\nTOK\n").unwrap();
        let v: Value = serde_json::from_str(&l).unwrap();
        assert_eq!(
            (v["access_key_id"].as_str(), v["session_token"].as_str()),
            (Some("AK"), Some("TOK"))
        );
        assert_eq!(
            credentials_from_text("AK").unwrap_err().code,
            "SO_SOURCE_BAD_CREDENTIAL"
        );
        assert!(credentials_from_text("{}").is_err());
    }

    #[test]
    fn a_credential_source_is_explicit_and_never_prompts_without_a_terminal() {
        let p = parsed(&["s3", "--name", "n"]);
        assert_eq!(credential_source(&p, false).unwrap_err().exit, exit::ARGS);
        assert_eq!(
            credential_source(&p, true).unwrap(),
            CredentialSource::Prompt
        );
        let p = parsed(&["--credentials-stdin"]);
        assert_eq!(
            credential_source(&p, false).unwrap(),
            CredentialSource::Stdin
        );
        let p = parsed(&["--secret-ref", "env:S3_CRED"]);
        assert_eq!(
            credential_source(&p, false).unwrap(),
            CredentialSource::Reference("env:S3_CRED".into())
        );
        assert!(credential_source(&parsed(&["--secret-ref", "relative/path"]), false).is_err());
        let p = parsed(&["--access-key-id-env", "A", "--secret-access-key-env", "S"]);
        assert!(matches!(
            credential_source(&p, false).unwrap(),
            CredentialSource::Env { .. }
        ));
        assert!(credential_source(&parsed(&["--access-key-id-env", "A"]), false).is_err());
        let p = parsed(&["--secret-ref", "env:X", "--credentials-stdin"]);
        assert!(credential_source(&p, false).is_err());
    }

    #[test]
    fn a_secret_value_flag_does_not_exist() {
        for flag in [
            "--secret-access-key",
            "--access-key-id",
            "--secret-key",
            "--password",
        ] {
            let a = vec![flag.to_owned(), "x".to_owned()];
            assert!(
                parse(&a, VALUED, SWITCHES).is_err(),
                "{flag} must be refused"
            );
        }
    }

    #[test]
    fn names_become_safe_file_names() {
        assert_eq!(slug("My Bucket/Prod"), "my-bucket-prod");
        assert_eq!(slug("--x--"), "x");
        assert!(oo_stack::stack::secret_name(&format!(
            "src_{}_s3",
            slug("Acme Docs")
        )));
    }

    #[test]
    fn the_public_view_hides_the_secret_reference() {
        let v = json!({"name": "n", "config": {"bucket": "b", "secret_ref": "file:/x/y"}});
        let p = public(&v);
        assert_eq!(p["config"]["secret_ref"], "(set)");
        assert!(!p.to_string().contains("/x/y"));
    }
}
