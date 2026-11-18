//! `oo agent token create|ls|revoke`: issue, list and revoke `ooa_` Agent tokens (D83).
//!
//! The token is shown once: it goes to a new 0600 file (`--output-file`) or, with `--print-once`, to
//! stdout. It is never part of the JSON report, a log or an argument.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::client::{CliError, Daemon, exit};
use super::docs_cmd::{Parsed, parse};
use super::secrets;

pub const HELP: &str = "Usage:\n  oo agent token create --name <n> --scope docs:read|docs:suggest|docs:write|db:read [--scope ...] [--drive <id|name>]... [--all-drives] [--gw-profile <profile>] [--ttl 7d] (--output-file <new file> | --print-once) [--json]\n  oo agent token ls [--json]\n  oo agent token revoke <name|id> [--json]\n  (all take --state-dir <dir>)\n";

fn bad(m: impl Into<String>) -> CliError {
    CliError::new(exit::ARGS, "DOCS_ENGINE_BAD_REQUEST", m)
}

/// `7d`, `12h`, `30m`, `90s` or plain seconds.
pub fn parse_ttl(s: &str) -> Option<u64> {
    let (n, unit) = match s.char_indices().last()? {
        (i, c) if c.is_ascii_alphabetic() => (&s[..i], c),
        _ => (s, 's'),
    };
    let n: u64 = n.parse().ok()?;
    n.checked_mul(match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        _ => return None,
    })
}

fn state_of(p: &Parsed) -> Result<PathBuf, CliError> {
    p.state_dir
        .clone()
        .or_else(|| std::env::var_os("OO_DOCS_STATE_DIR").map(PathBuf::from))
        .or_else(secrets::default_state_dir)
        .ok_or_else(|| bad("no state directory: set --state-dir or HOME"))
}

async fn drive_ids(d: &Daemon, wanted: &[&str], all: bool) -> Result<Vec<String>, CliError> {
    if all || wanted.is_empty() {
        return Ok(Vec::new());
    }
    let list = d.get_json("/v1/docs/drives?limit=200").await?;
    wanted
        .iter()
        .map(|w| {
            list["items"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|i| {
                    i["id"] == *w
                        || i["name"]
                            .as_str()
                            .is_some_and(|n| n.eq_ignore_ascii_case(w))
                })
                .and_then(|i| i["id"].as_str().map(str::to_owned))
                .ok_or_else(|| {
                    CliError::new(
                        exit::OTHER,
                        "DOCS_SOURCE_DRIVE_NOT_FOUND",
                        format!("no drive {w}"),
                    )
                })
        })
        .collect()
}

async fn create(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let name = p.opt("--name").ok_or_else(|| bad("--name is required"))?;
    let scopes = p.all("--scope");
    if scopes.is_empty() {
        return Err(bad("at least one --scope is required"));
    }
    let all = p.has("--all-drives");
    let mut body = json!({"name": name, "scopes": scopes});
    let drives = drive_ids(d, &p.all("--drive"), all).await?;
    if all {
        body["all_drives"] = json!(true);
    } else if !drives.is_empty() {
        body["drive_ids"] = json!(drives);
    } else if scopes.iter().any(|s| s.starts_with("docs:")) {
        return Err(bad("docs scopes need --drive <id|name> or --all-drives"));
    }
    if let Some(g) = p.opt("--gw-profile") {
        body["gw_profile"] = json!(g);
    }
    if let Some(t) = p.opt("--ttl") {
        body["expires_in_seconds"] =
            json!(parse_ttl(t).ok_or_else(|| bad("--ttl is like 90s, 30m, 12h or 7d"))?);
    }
    let out = p.opt("--output-file").map(PathBuf::from);
    if out.is_none() == !p.has("--print-once") {
        return Err(bad(
            "give exactly one of --output-file <new file> and --print-once",
        ));
    }
    if let Some(f) = &out {
        if !f.is_absolute() {
            return Err(bad("--output-file must be an absolute path"));
        }
        if std::fs::symlink_metadata(f).is_ok() {
            return Err(bad("--output-file exists; the token goes to a new file"));
        }
    }
    let r = d.post_json("/v1/docs/agent-tokens", &body).await?;
    let token = r["token"].as_str().ok_or_else(|| {
        CliError::new(
            exit::OTHER,
            "DOCS_ENGINE_INTERNAL",
            "no token in the answer",
        )
    })?;
    if let Some(f) = &out {
        secrets::write_secret_file(f, &format!("{token}\n")).map_err(|e| {
            CliError::new(
                exit::OTHER,
                "DOCS_ENGINE_INTERNAL",
                format!("cannot write the token file: {e}"),
            )
        })?;
    }
    let mut rep = json!({"protocol": "oo.agent-token/v1", "id": r["id"], "name": r["name"], "scopes": r["scopes"],
        "drive_ids": r["drive_ids"], "all_drives": r["all_drives"], "gw_profile": r["gw_profile"], "expires_at": r["expires_at"]});
    if let Some(f) = &out {
        rep["token_file"] = json!(f.display().to_string());
    }
    if p.json {
        println!("{rep}");
        if out.is_none() {
            // A second line, so a report parser and a token reader stay apart.
            println!("{token}");
        }
    } else {
        println!(
            "agent token {} created ({})",
            r["name"].as_str().unwrap_or(""),
            r["id"].as_str().unwrap_or("")
        );
        match &out {
            Some(f) => println!("token file: {}", f.display()),
            None => println!("{token}"),
        }
    }
    Ok(())
}

async fn ls(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let v = d.get_json("/v1/docs/agent-tokens").await?;
    if p.json {
        println!(
            "{}",
            json!({"protocol": "oo.agent-token-list/v1", "items": v["items"]})
        );
    } else {
        for t in v["items"].as_array().into_iter().flatten() {
            println!(
                "{}  {}  {}  expires {}",
                t["id"].as_str().unwrap_or(""),
                t["name"].as_str().unwrap_or(""),
                t["scopes"],
                t["expires_at"].as_str().unwrap_or("")
            );
        }
    }
    Ok(())
}

async fn revoke(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let want = p
        .pos
        .first()
        .ok_or_else(|| bad("revoke needs a token name or id"))?;
    let v = d.get_json("/v1/docs/agent-tokens").await?;
    let t: &Value = v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["id"] == *want || t["name"] == *want)
        .ok_or_else(|| {
            CliError::new(
                exit::OTHER,
                "DOCS_ENGINE_NOT_FOUND",
                format!("no agent token {want}"),
            )
        })?;
    let id = t["id"].as_str().unwrap_or("");
    d.delete(&format!("/v1/docs/agent-tokens/{id}"), None)
        .await?;
    if p.json {
        println!(
            "{}",
            json!({"protocol": "oo.agent-token-revoked/v1", "id": id, "revoked": true})
        );
    } else {
        println!("agent token {want} revoked");
    }
    Ok(())
}

/// Entry for `oo agent <rest...>`; returns the exit code.
pub fn run(rest: &[String]) -> u8 {
    if rest.first().map(String::as_str) != Some("token") || rest.len() < 2 {
        eprint!("{HELP}");
        return exit::ARGS;
    }
    let p = match parse(
        &rest[2..],
        &[
            "--name",
            "--scope",
            "--drive",
            "--gw-profile",
            "--ttl",
            "--output-file",
        ],
        &["--all-drives", "--print-once", "--yes"],
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("oo agent token: {e}");
            return exit::ARGS;
        }
    };
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return exit::OTHER;
    };
    let res = rt.block_on(async {
        let d = Daemon::connect(&state_of(&p)?)?;
        match rest[1].as_str() {
            "create" => create(&d, &p).await,
            "ls" => ls(&d, &p).await,
            "revoke" => revoke(&d, &p).await,
            _ => Err(bad("unknown subcommand; see `oo agent --help`")),
        }
    });
    match res {
        Ok(()) => exit::OK,
        Err(e) => {
            eprintln!(
                "{}",
                json!({"protocol": "oo.docs-error/v1", "error": {"code": e.code, "message": e.message}})
            );
            e.exit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn ttl_units() {
        assert_eq!(parse_ttl("7d"), Some(604800));
        assert_eq!(parse_ttl("90"), Some(90));
        assert_eq!(parse_ttl("2h"), Some(7200));
        assert_eq!(parse_ttl("x"), None);
        assert_eq!(parse_ttl("5w"), None);
    }

    #[test]
    fn token_files_are_private() {
        let f = std::env::temp_dir().join(format!("oo-agent-tok-{}", std::process::id()));
        let _ = std::fs::remove_file(&f);
        secrets::write_secret_file(&f, "ooa_x\n").unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().mode() & 0o777, 0o600);
        let _ = std::fs::remove_file(&f);
    }
}
