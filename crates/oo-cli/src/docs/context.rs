//! `oo context mount` (10 §7.9, L2 subset): compile a pack for a scope, create
//! the share and write `<dir>/.context` atomically and read-only.
//!
//! Flags: `--drive`, `--scope` (repeatable), `--to`, `--for` / `--for-file`,
//! `--seed` (repeatable), `--budget`, `--role`, `--expires`, `--version`,
//! `--strict-budget`, `--dry-run`, `--json`, `--state-dir`. `list`, `status`,
//! `refresh`, `revoke`, `check`, `--group`, `--refresh-policy` and
//! `--require-complete` are here too. The control token and the refs credential
//! are never printed.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::client::{CliError, Daemon, encode_path, exit};
use super::mounts::{self, MountError};

/// Parsed `oo context mount` arguments.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MountArgs {
    pub drive: Option<String>,
    pub scope: Vec<String>,
    pub to: Option<String>,
    pub task: Option<String>,
    pub task_file: Option<String>,
    pub seeds: Vec<String>,
    pub budget: Option<u32>,
    pub role: Option<String>,
    pub expires_secs: Option<i64>,
    pub version: Option<i64>,
    pub strict_budget: bool,
    pub dry_run: bool,
    pub json: bool,
    pub state_dir: Option<PathBuf>,
    pub group: Option<String>,
    pub refresh_policy: Option<String>,
    pub require_complete: bool,
    pub wait_ms: Option<u64>,
}

/// `7d`, `24h`, `90m` or plain seconds.
pub fn parse_duration(text: &str) -> Option<i64> {
    let (num, unit) = match text.char_indices().last()? {
        (i, 'd') => (&text[..i], 86_400),
        (i, 'h') => (&text[..i], 3_600),
        (i, 'm') => (&text[..i], 60),
        (i, 's') => (&text[..i], 1),
        _ => (text, 1),
    };
    num.parse::<i64>().ok().filter(|n| *n > 0).map(|n| n * unit)
}

/// Parse the arguments after `mount`.
pub fn parse_mount_args(rest: &[String]) -> Result<MountArgs, String> {
    let mut a = MountArgs::default();
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_owned())),
            _ => (arg.as_str(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| it.next().cloned())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag {
            "--drive" => a.drive = Some(value("--drive")?),
            "--scope" => a.scope.push(value("--scope")?),
            "--to" => a.to = Some(value("--to")?),
            "--for" => a.task = Some(value("--for")?),
            "--for-file" => a.task_file = Some(value("--for-file")?),
            "--seed" => a.seeds.push(value("--seed")?),
            "--budget" => {
                let n: u32 = value("--budget")?
                    .parse()
                    .map_err(|_| "--budget must be a number".to_owned())?;
                if !(200..=20_000).contains(&n) {
                    return Err("--budget must be 200..=20000".into());
                }
                a.budget = Some(n);
            }
            "--role" => {
                let r = value("--role")?;
                if !matches!(r.as_str(), "viewer" | "suggester" | "editor") {
                    return Err("--role must be viewer, suggester or editor".into());
                }
                a.role = Some(r);
            }
            "--expires" => {
                let secs = parse_duration(&value("--expires")?)
                    .filter(|s| (3_600..=2_592_000).contains(s))
                    .ok_or("--expires must be between 1h and 30d, like 7d or 24h")?;
                a.expires_secs = Some(secs);
            }
            "--version" => {
                a.version = Some(
                    value("--version")?
                        .parse::<i64>()
                        .ok()
                        .filter(|n| *n >= 1)
                        .ok_or("--version must be a positive number")?,
                );
            }
            "--state-dir" => a.state_dir = Some(PathBuf::from(value("--state-dir")?)),
            "--group" => {
                let g = value("--group")?;
                if g.is_empty()
                    || g.len() > 128
                    || !g
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._#:-".contains(c))
                {
                    return Err("--group must match [A-Za-z0-9._#:-]{1,128}".into());
                }
                a.group = Some(g);
            }
            "--refresh-policy" => {
                let p = value("--refresh-policy")?;
                if !matches!(p.as_str(), "manual" | "at-checkpoint" | "pinned") {
                    return Err("--refresh-policy must be manual, at-checkpoint or pinned".into());
                }
                a.refresh_policy = Some(p);
            }
            "--require-complete" => a.require_complete = true,
            "--wait" => {
                let text = value("--wait")?;
                let secs = if matches!(text.as_str(), "0" | "0s") {
                    Some(0)
                } else {
                    parse_duration(&text)
                };
                let secs = secs
                    .filter(|s| *s <= 120)
                    .ok_or("--wait must be at most 120s, like 20s or 0s")?;
                a.wait_ms = Some(secs as u64 * 1000);
            }
            "--strict-budget" => a.strict_budget = true,
            "--dry-run" => a.dry_run = true,
            "--json" => a.json = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if a.task.is_some() && a.task_file.is_some() {
        return Err("--for and --for-file are exclusive".into());
    }
    if a.to.is_none() && !a.dry_run {
        return Err("--to <dir> is required".into());
    }
    Ok(a)
}

fn fail(e: &CliError, json: bool) -> u8 {
    if json {
        eprintln!(
            "{}",
            json!({"protocol": "oo.context-mount/v1", "error": {"code": e.code, "message": e.message}})
        );
    } else {
        eprintln!("oo context: {} ({})", e.message, e.code);
    }
    e.exit
}

/// Entry for `oo context <rest...>`; returns the exit code.
pub fn run(rest: &[String]) -> u8 {
    let Some((sub, tail)) = rest.split_first() else {
        eprintln!("oo context: expected a subcommand: mount, list, status, refresh, revoke, check");
        return exit::ARGS;
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return exit::OTHER,
    };
    match sub.as_str() {
        "mount" => {
            let args = match parse_mount_args(tail) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("oo context mount: {e}");
                    return exit::ARGS;
                }
            };
            match rt.block_on(mount(&args)) {
                Ok(()) => exit::OK,
                Err(e) => fail(&e, args.json),
            }
        }
        "list" | "status" | "refresh" | "revoke" | "check" | "collect" => {
            let a = match parse_simple(sub, tail) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("oo context {sub}: {e}");
                    return exit::ARGS;
                }
            };
            let out = match sub.as_str() {
                "list" => rt.block_on(list(&a)),
                "status" => rt.block_on(status(&a)),
                "refresh" => rt.block_on(refresh(&a)),
                "revoke" => rt.block_on(revoke(&a)),
                "collect" => rt.block_on(collect(&a)),
                _ => return rt.block_on(check(&a)),
            };
            match out {
                Ok(()) => exit::OK,
                Err(e) => fail(&e, a.json),
            }
        }
        other => {
            eprintln!("oo context: unknown subcommand `{other}`");
            exit::ARGS
        }
    }
}

/// Arguments of `list`, `status`, `refresh`, `revoke` and `check`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SimpleArgs {
    /// Share id or directory (`status`, `refresh`, `revoke`).
    pub target: Option<String>,
    pub json: bool,
    pub state_dir: Option<PathBuf>,
    pub drive: Option<String>,
    pub status: Option<String>,
    /// `check`: the `.context` directory (default `./.context`).
    pub dir: Option<PathBuf>,
    pub since: Option<i64>,
    /// `refresh --group <ticket>`.
    pub group: Option<String>,
}

/// Parse the arguments after the subcommand.
pub fn parse_simple(sub: &str, rest: &[String]) -> Result<SimpleArgs, String> {
    let mut a = SimpleArgs::default();
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_owned())),
            _ => (arg.as_str(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| it.next().cloned())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match (sub, flag) {
            (_, "--json") => a.json = true,
            (_, "--state-dir") => a.state_dir = Some(PathBuf::from(value("--state-dir")?)),
            ("list", "--drive") => a.drive = Some(value("--drive")?),
            ("list", "--status") => {
                let s = value("--status")?;
                if !matches!(
                    s.as_str(),
                    "active" | "drifted" | "refreshed" | "expired" | "revoked"
                ) {
                    return Err("--status is active, drifted, refreshed, expired or revoked".into());
                }
                a.status = Some(s);
            }
            ("check", "--dir") => a.dir = Some(PathBuf::from(value("--dir")?)),
            ("check", "--since") => {
                a.since = Some(
                    value("--since")?
                        .parse::<i64>()
                        .ok()
                        .filter(|n| *n >= 0)
                        .ok_or("--since must be a sequence number")?,
                );
            }
            ("refresh", "--group") => a.group = Some(value("--group")?),
            (_, f) if f.starts_with("--") => return Err(format!("unknown option {f}")),
            ("status" | "refresh" | "revoke" | "collect", _) if a.target.is_none() => {
                a.target = Some(arg.clone());
            }
            _ => return Err(format!("unexpected argument {arg}")),
        }
    }
    if matches!(sub, "status" | "revoke" | "collect") && a.target.is_none() {
        return Err("expected <share-id|dir>".into());
    }
    if sub == "refresh" && a.target.is_none() && a.group.is_none() {
        return Err("expected <share-id|dir> or --group <ticket>".into());
    }
    if sub == "refresh" && a.target.is_some() && a.group.is_some() {
        return Err("<share-id|dir> and --group are exclusive".into());
    }
    Ok(a)
}

fn state_of(a: &SimpleArgs) -> Result<PathBuf, CliError> {
    a.state_dir
        .clone()
        .or_else(|| std::env::var_os("OO_DOCS_STATE_DIR").map(PathBuf::from))
        .or_else(super::secrets::default_state_dir)
        .ok_or_else(|| {
            CliError::new(
                exit::ARGS,
                "DOCS_ENGINE_BAD_REQUEST",
                "no state directory: set --state-dir or HOME",
            )
        })
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// A share id and, when known, the directory holding its mount.
fn resolve_target(state: &Path, target: &str) -> Result<(String, Option<PathBuf>), CliError> {
    if is_uuid(target) {
        let m = mounts::load_mounts(state);
        let dir = m
            .values()
            .find(|e| e["share_id"] == target)
            .and_then(|e| e["path"].as_str().map(PathBuf::from));
        return Ok((target.to_owned(), dir));
    }
    // A directory: the target itself, or one holding `.context`.
    let p = PathBuf::from(target);
    let base = if p.file_name().is_some_and(|n| n == mounts::MOUNT_DIR) {
        p.parent().map(Path::to_path_buf).unwrap_or(p)
    } else {
        p
    };
    let sid = mounts::share_id_in(&base).ok_or_else(|| {
        CliError::new(
            exit::SHARE_STATE,
            "DOCS_SHARE_NOT_FOUND",
            "no share id: neither a UUID nor a directory with .context/SHARE.json",
        )
    })?;
    Ok((sid, Some(base)))
}

fn emit(json: bool, protocol: &str, mut v: Value, human: String) {
    if json {
        v["protocol"] = json!(protocol);
        println!("{v}");
    } else {
        println!("{human}");
    }
}

async fn list(a: &SimpleArgs) -> Result<(), CliError> {
    let state = state_of(a)?;
    let d = Daemon::connect(&state)?;
    let mut path = String::from("/v1/docs/shares?limit=200");
    if let Some(s) = &a.status {
        path.push_str(&format!("&state={s}"));
    }
    if a.drive.is_some() {
        let drive = resolve_drive(&d, a.drive.as_deref()).await?;
        path.push_str(&format!("&drive_id={drive}"));
    }
    let l = d.get_json(&path).await?;
    let items = l["items"].as_array().cloned().unwrap_or_default();
    let mounts = mounts::load_mounts(&state);
    let dir_of = |sid: &str| -> Option<String> {
        mounts
            .values()
            .find(|e| e["share_id"] == sid)
            .and_then(|e| e["path"].as_str().map(str::to_owned))
    };
    if a.json {
        let enriched: Vec<Value> = items
            .iter()
            .map(|s| {
                let mut s = s.clone();
                s["path"] = json!(s["id"].as_str().and_then(dir_of));
                s
            })
            .collect();
        println!(
            "{}",
            json!({"protocol": "oo.context-list/v1", "shares": enriched})
        );
    } else if items.is_empty() {
        println!("no shares");
    } else {
        for s in &items {
            let sid = s["id"].as_str().unwrap_or("");
            println!(
                "{sid}  {:<9} v{}  expires {}  {}",
                s["state"].as_str().unwrap_or(""),
                s["version"],
                s["expires_at"].as_str().unwrap_or(""),
                dir_of(sid).unwrap_or_else(|| "-".into())
            );
        }
    }
    Ok(())
}

/// `oo context collect <dir|share-id>`: collect `.context/suggest/` once, by hand.
async fn collect(a: &SimpleArgs) -> Result<(), CliError> {
    let state = state_of(a)?;
    let d = Daemon::connect(&state)?;
    let target = a.target.clone().unwrap_or_default();
    let (sid, dir) = resolve_target(&state, &target)?;
    let dir = dir.ok_or_else(|| {
        CliError::new(
            exit::SHARE_STATE,
            "DOCS_SHARE_NOT_FOUND",
            "this share has no mount directory on this machine",
        )
    })?;
    let v = super::suggest::collect_dir(
        &d,
        &sid,
        &dir.join(mounts::MOUNT_DIR).join(super::suggest::DIR),
    )
    .await?;
    let human = format!(
        "collected {}, rejected {}, left alone {}",
        v["collected"], v["rejected"], v["skipped"]
    );
    emit(a.json, "oo.context-collect/v1", v, human);
    Ok(())
}

async fn status(a: &SimpleArgs) -> Result<(), CliError> {
    let state = state_of(a)?;
    let target = a.target.as_deref().unwrap_or("");
    let (sid, _dir) = resolve_target(&state, target)?;
    let d = Daemon::connect(&state)?;
    let s = d.get_json(&format!("/v1/docs/shares/{sid}")).await?;
    let ups = d
        .get_json(&format!("/v1/docs/shares/{sid}/updates?limit=1000"))
        .await?;
    let items = ups["items"].as_array().cloned().unwrap_or_default();
    let worst = ["block", "warn", "info"]
        .into_iter()
        .find(|l| items.iter().any(|i| i["level"] == *l));
    let mut v = s.clone();
    v["updates"] = json!({"count": items.len(), "highest_level": worst});
    let human = format!(
        "share {sid}: {} (pack v{}), {} updates{}, expires {}",
        s["state"].as_str().unwrap_or(""),
        s["version"],
        items.len(),
        worst.map(|l| format!(", highest {l}")).unwrap_or_default(),
        s["expires_at"].as_str().unwrap_or("")
    );
    emit(a.json, "oo.context-status/v1", v, human);
    Ok(())
}

/// Mount a refreshed share over the old one in `dir`.
async fn remount(
    d: &Daemon,
    state: &Path,
    dir: &Path,
    old: &str,
    new_share: &Value,
) -> Result<PathBuf, CliError> {
    let files = pack_files(d, new_share).await?;
    let sid = str_of(new_share, "id")?;
    let mut files = files;
    super::suggest::add_dir(&mut files, new_share["role"].as_str().unwrap_or("viewer"));
    let at = mounts::replace(dir, &sid, old, &files).map_err(|e| target_error(&e))?;
    super::suggest::open_dir(&at);
    if let Some(mref) = new_share["mount_ref"].as_str() {
        let _ = mounts::register(state, mref, &sid, dir);
    }
    Ok(at)
}

async fn refresh(a: &SimpleArgs) -> Result<(), CliError> {
    let state = state_of(a)?;
    let d = Daemon::connect(&state)?;
    // (old share, dir, new share) per refreshed member.
    let mut done: Vec<(String, Option<PathBuf>, Value)> = Vec::new();
    if let Some(ticket) = &a.group {
        let mount_dirs = mounts::load_mounts(&state);
        let r = d
            .post_json(
                &format!("/v1/docs/share-groups/{ticket}:refresh"),
                &json!({}),
            )
            .await?;
        let results = r["results"].as_array().cloned().unwrap_or_default();
        let fresh = r["refreshed"].as_array().cloned().unwrap_or_default();
        for (res, new) in results.iter().zip(fresh) {
            let old = res["id"].as_str().unwrap_or("").to_owned();
            let dir = mount_dirs
                .values()
                .find(|e| e["share_id"] == old.as_str())
                .and_then(|e| e["path"].as_str().map(PathBuf::from));
            done.push((old, dir, new));
        }
    } else {
        let (sid, dir) = resolve_target(&state, a.target.as_deref().unwrap_or(""))?;
        let new = d
            .post_json(&format!("/v1/docs/shares/{sid}:refresh"), &json!({}))
            .await?;
        done.push((sid, dir, new));
    }
    let mut report = Vec::new();
    for (old, dir, new) in &done {
        let nsid = str_of(new, "id")?;
        let path = match dir {
            Some(dir) => Some(remount(&d, &state, dir, old, new).await?),
            None => None,
        };
        report.push(
            json!({"old": old, "share_id": nsid, "version": new["version"],
                           "path": path, "mounted": path.is_some()}),
        );
    }
    let human: Vec<String> = report
        .iter()
        .map(|r| {
            format!(
                "refreshed {} -> {} (v{}){}",
                r["old"].as_str().unwrap_or(""),
                r["share_id"].as_str().unwrap_or(""),
                r["version"],
                r["path"]
                    .as_str()
                    .map(|p| format!(", mounted at {p}"))
                    .unwrap_or_else(|| ", no local mount known".into())
            )
        })
        .collect();
    emit(
        a.json,
        "oo.context-refresh/v1",
        json!({"shares": report}),
        human.join("\n"),
    );
    Ok(())
}

async fn revoke(a: &SimpleArgs) -> Result<(), CliError> {
    let state = state_of(a)?;
    let (sid, dir) = resolve_target(&state, a.target.as_deref().unwrap_or(""))?;
    let d = Daemon::connect(&state)?;
    let r = d
        .post_json(&format!("/v1/docs/shares/{sid}:revoke"), &json!({}))
        .await?;
    let removed = dir
        .as_deref()
        .is_some_and(|dir| mounts::remove_if_ours(dir, &sid));
    let mut m = mounts::load_mounts(&state);
    let before = m.len();
    m.retain(|_, e| e["share_id"] != sid.as_str());
    if m.len() != before {
        let _ = mounts::save_mounts(&state, &m);
    }
    let v = json!({"share_id": sid, "state": r["share"]["state"], "local_removed": removed,
                   "notice_code": r["notice_code"]});
    let human = format!(
        "revoked {sid}; local mount {}. Content already read cannot be taken back.",
        if removed {
            "removed"
        } else {
            "not removed (not found or not ours)"
        }
    );
    emit(a.json, "oo.context-revoke/v1", v, human);
    Ok(())
}

/// Exit codes of `check` (00 5.6.3).
pub mod check_exit {
    pub const OK: u8 = 0;
    pub const WARN: u8 = 10;
    pub const BLOCK: u8 = 20;
    pub const ENDED: u8 = 30;
    pub const UNREACHABLE: u8 = 40;
}

/// Turn the engine's answer into the exit code of the gate; anything it does
/// not understand closes the gate.
pub fn check_code(status: u16, body: &Value) -> u8 {
    match status {
        200 => match body["exit_code"].as_u64() {
            Some(0) => check_exit::OK,
            Some(10) => check_exit::WARN,
            Some(20) => check_exit::BLOCK,
            Some(30) => check_exit::ENDED,
            _ => check_exit::UNREACHABLE,
        },
        // Expired, revoked or refreshed: the credential is dead.
        410 => check_exit::ENDED,
        _ => check_exit::UNREACHABLE,
    }
}

async fn check(a: &SimpleArgs) -> u8 {
    let report = |code: u8, mut v: Value, human: String| -> u8 {
        if a.json {
            v["protocol"] = json!("oo.context-check/v1");
            v["exit_code"] = json!(code);
            println!("{v}");
        } else {
            println!("{human}");
        }
        code
    };
    let dir = a.dir.clone().unwrap_or_else(|| PathBuf::from(".context"));
    let sj: Value = match std::fs::read_to_string(dir.join("SHARE.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(v) => v,
        None => {
            return report(
                check_exit::UNREACHABLE,
                json!({"error": "no readable SHARE.json"}),
                format!(
                    "oo context check: {}/SHARE.json is missing or unreadable; treating the gate as closed",
                    dir.display()
                ),
            );
        }
    };
    let (Some(sid), Some(cred), Some(endpoint)) = (
        sj["share_id"].as_str(),
        sj["refs"]["credential"].as_str(),
        sj["refs"]["endpoint"].as_str(),
    ) else {
        return report(
            check_exit::UNREACHABLE,
            json!({"error": "SHARE.json lacks the share id or credential"}),
            "oo context check: SHARE.json is incomplete; treating the gate as closed".into(),
        );
    };
    let base = endpoint.split("/v1/").next().unwrap_or(endpoint);
    let d = match Daemon::with_credential(base, cred) {
        Ok(d) => d,
        Err(_) => {
            return report(
                check_exit::UNREACHABLE,
                json!({}),
                "oo context check: no http client".into(),
            );
        }
    };
    // The at-checkpoint upgrade needs the control token to pull the new pack;
    // without one (an Agent sandbox) the engine only recommends it.
    let state = a
        .state_dir
        .clone()
        .or_else(|| std::env::var_os("OO_DOCS_STATE_DIR").map(PathBuf::from))
        .or_else(super::secrets::default_state_dir);
    let control = state.as_deref().and_then(|s| Daemon::connect(s).ok());
    let mut query: Vec<String> = Vec::new();
    if let Some(s) = a.since {
        query.push(format!("since={s}"));
    }
    if control.is_some() {
        query.push("auto_refresh=true".into());
    }
    let mut path = format!("/v1/docs/shares/{sid}:check");
    if !query.is_empty() {
        path.push_str(&format!("?{}", query.join("&")));
    }
    let (status, body) = match d.post_raw(&path, &json!({})).await {
        Ok(x) => x,
        Err(_) => {
            return report(
                check_exit::UNREACHABLE,
                json!({"share_id": sid, "action": "stop"}),
                "oo context check: the engine is not reachable (is `oo serve` running?); treat this as blocked".into(),
            );
        }
    };
    let code = check_code(status, &body);
    let mut out = json!({"share_id": sid, "level": body["level"], "items": body["items"],
                         "action": body["action"]});
    if status == 410 {
        out["action"] = json!("stop_share_ended");
        out["reason"] = body["details"]["reason"].clone();
    }
    let mut human = match code {
        0 => format!("share {sid}: up to date, continue"),
        10 => format!("share {sid}: facts you used were superseded (warn); read UPDATES.md and refresh before committing"),
        20 => format!("share {sid}: BLOCK, a fact you used was withdrawn or conflicts; do not commit, refresh or ask a person"),
        30 => format!("share {sid}: the share has ended (expired, revoked or refreshed); stop using this pack"),
        _ => "oo context check: the engine answered something unexpected; treating the gate as closed".into(),
    };
    // at-checkpoint: the engine refreshed before answering; mount the new pack here.
    if let Some(new) = body.get("refreshed").filter(|v| v.is_object()) {
        let target = dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let target = if target.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            target
        };
        let Some(control) = control.as_ref() else {
            return report(code, out, human);
        };
        match pack_files(control, new).await {
            Ok(mut files) => {
                let nsid = new["id"].as_str().unwrap_or("");
                super::suggest::add_dir(&mut files, new["role"].as_str().unwrap_or("viewer"));
                match mounts::replace(
                    &std::fs::canonicalize(&target).unwrap_or(target),
                    nsid,
                    sid,
                    &files,
                ) {
                    Ok(at) => {
                        super::suggest::open_dir(&at);
                        out["refreshed_to"] = json!(nsid);
                        out["upgraded_to_version"] = body["upgraded_to_version"].clone();
                        human.push_str(&format!(
                            "\n已升级到 v{} (share {nsid})",
                            body["upgraded_to_version"]
                        ));
                    }
                    Err(e) => human.push_str(&format!(
                        "\n自动刷新已在引擎完成，但本地目录未能替换：{e:?}"
                    )),
                }
            }
            Err(e) => human.push_str(&format!(
                "\n自动刷新已在引擎完成，但拉取新 pack 失败：{}",
                e.message
            )),
        }
    }
    report(code, out, human)
}

async fn resolve_drive(d: &Daemon, want: Option<&str>) -> Result<String, CliError> {
    let list = d.get_json("/v1/docs/drives?limit=200").await?;
    let items = list["items"].as_array().cloned().unwrap_or_default();
    let pick = |hit: Option<&Value>| hit.and_then(|v| v["id"].as_str()).map(str::to_owned);
    match want {
        Some(w) => pick(items.iter().find(|i| {
            i["id"] == w
                || i["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(w))
        }))
        .ok_or_else(|| CliError::new(exit::OTHER, "DOCS_SOURCE_DRIVE_NOT_FOUND", "no such drive")),
        None if items.len() == 1 => Ok(pick(items.first()).unwrap_or_default()),
        None if items.is_empty() => Err(CliError::new(
            exit::OTHER,
            "DOCS_SOURCE_DRIVE_NOT_FOUND",
            "there is no drive; register one first",
        )),
        None => Err(CliError::new(
            exit::ARGS,
            "DOCS_ENGINE_BAD_REQUEST",
            "several drives exist; pick one with --drive <id|name>",
        )),
    }
}

fn compile_body(args: &MountArgs, drive: &str, task: Option<&str>) -> Value {
    let mut c = json!({"drive_id": drive, "role": args.role.as_deref().unwrap_or("suggester")});
    if !args.scope.is_empty() {
        c["scope"] = json!(args.scope);
    }
    if let Some(t) = task {
        c["task"] = json!(t);
    }
    if !args.seeds.is_empty() {
        c["seeds"] = json!(args.seeds);
    }
    if let Some(b) = args.budget {
        c["budget"] = json!(b);
    }
    if let Some(v) = args.version {
        c["version"] = json!(v);
    }
    if args.require_complete {
        c["require_complete"] = json!(true);
    }
    if let Some(w) = args.wait_ms {
        c["wait_ms"] = json!(w);
    }
    c
}

/// Sum of every trim counter of a `trimmed` object.
pub fn trimmed_total(trimmed: &Value) -> u64 {
    trimmed
        .as_object()
        .map(|o| {
            o.values()
                .flat_map(|v| v.as_object().into_iter().flat_map(|m| m.values()))
                .filter_map(Value::as_u64)
                .sum()
        })
        .unwrap_or(0)
}

fn state_dir(args: &MountArgs) -> Result<PathBuf, CliError> {
    args.state_dir
        .clone()
        .or_else(|| std::env::var_os("OO_DOCS_STATE_DIR").map(PathBuf::from))
        .or_else(super::secrets::default_state_dir)
        .ok_or_else(|| {
            CliError::new(
                exit::ARGS,
                "DOCS_ENGINE_BAD_REQUEST",
                "no state directory: set --state-dir or HOME",
            )
        })
}

fn read_task(args: &MountArgs) -> Result<Option<String>, CliError> {
    let text = match (&args.task, &args.task_file) {
        (Some(t), _) => t.clone(),
        (None, Some(f)) => std::fs::read_to_string(f).map_err(|_| {
            CliError::new(
                exit::ARGS,
                "DOCS_ENGINE_BAD_REQUEST",
                "--for-file cannot be read",
            )
        })?,
        (None, None) => return Ok(None),
    };
    let text = text.trim().to_owned();
    if text.chars().count() > 2000 {
        return Err(CliError::new(
            exit::ARGS,
            "DOCS_PACK_TASK_TOO_LONG",
            "the task is longer than 2000 characters",
        ));
    }
    Ok((!text.is_empty()).then_some(text))
}

fn target_error(e: &MountError) -> CliError {
    match e {
        MountError::Occupied => CliError::new(
            exit::TARGET,
            "DOCS_SHARE_TARGET_OCCUPIED",
            "the target already has a .context; remove it yourself or pick another directory",
        ),
        MountError::Invalid(why) => CliError::new(exit::TARGET, "DOCS_SHARE_TARGET_INVALID", *why),
        MountError::BadPath => CliError::new(
            exit::OTHER,
            "DOCS_SHARE_TARGET_WRITE_FAILED",
            "the pack names an unsafe file path",
        ),
        MountError::Write(_) => CliError::new(
            exit::OTHER,
            "DOCS_SHARE_TARGET_WRITE_FAILED",
            "writing the mount failed; nothing was left behind",
        ),
    }
}

async fn mount(args: &MountArgs) -> Result<(), CliError> {
    let state = state_dir(args)?;
    let task = read_task(args)?;
    let daemon = Daemon::connect(&state)?;

    // Validate the target before asking the engine to create anything.
    let target = match &args.to {
        Some(to) => {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let real = mounts::check_target(to, home.as_deref(), Some(&state))
                .map_err(|e| target_error(&e))?;
            if real.join(mounts::MOUNT_DIR).symlink_metadata().is_ok() {
                return Err(target_error(&MountError::Occupied));
            }
            Some(real)
        }
        None => None,
    };

    let drive = resolve_drive(&daemon, args.drive.as_deref()).await?;
    let compile = compile_body(args, &drive, task.as_deref());

    // Compile first when it decides what happens next: a dry run, or a strict
    // budget that must not create a share for a trimmed pack. The pack is
    // cached, so the share below reuses it.
    if args.dry_run || args.strict_budget {
        let pack = daemon
            .post_json("/v1/docs/context-packs:compile", &compile)
            .await?;
        let trimmed = trimmed_total(&pack["trimmed"]);
        if args.dry_run {
            report_dry(args, &pack, trimmed);
            return Ok(());
        }
        if trimmed > 0 {
            return Err(CliError::new(
                exit::STRICT_BUDGET,
                "DOCS_PACK_BUDGET_TRIMMED",
                format!("the pack had to trim {trimmed} items to fit the budget (--strict-budget)"),
            ));
        }
    }
    let Some(target) = target else { return Ok(()) };

    let mut body = json!({"compile": compile, "mount": {"target_dir": target}});
    if let Some(s) = args.expires_secs {
        body["expires_in_seconds"] = json!(s);
    }
    if let Some(g) = &args.group {
        body["group"] = json!(g);
    }
    if let Some(p) = &args.refresh_policy {
        body["refresh_policy"] = json!(p);
    }
    let share = daemon.post_json("/v1/docs/shares", &body).await?;
    let sid = str_of(&share, "id")?;
    let files = match pack_files(&daemon, &share).await {
        Ok(mut f) => {
            super::suggest::add_dir(&mut f, share["role"].as_str().unwrap_or("viewer"));
            f
        }
        Err(e) => {
            // Nothing was mounted: do not leave a live share behind.
            let _ = daemon
                .post_json(&format!("/v1/docs/shares/{sid}:revoke"), &json!({}))
                .await;
            return Err(e);
        }
    };
    let mounted = match mounts::install(&target, &sid, &files) {
        Ok(m) => m,
        Err(e) => {
            let _ = daemon
                .post_json(&format!("/v1/docs/shares/{sid}:revoke"), &json!({}))
                .await;
            return Err(target_error(&e));
        }
    };
    super::suggest::open_dir(&mounted);
    if let Some(mref) = share["mount_ref"].as_str() {
        let _ = mounts::register(&state, mref, &sid, &target);
    }
    report_done(args, &share, &mounted);
    Ok(())
}

/// Pull every file of a share's pack, verify it against the listed sha256 and
/// size, add `SHARE.json`, sorted by path.
async fn pack_files(d: &Daemon, share: &Value) -> Result<Vec<(String, Vec<u8>)>, CliError> {
    let pid = str_of(share, "pack_id")?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for f in share["files"].as_array().cloned().unwrap_or_default() {
        let path = str_of(&f, "path")?;
        let bytes = d
            .get_bytes(&format!(
                "/v1/docs/context-packs/{pid}/files/{}",
                encode_path(&path)
            ))
            .await?;
        let want = str_of(&f, "sha256")?;
        let got = hex(&Sha256::digest(&bytes));
        if got != want || f["bytes"].as_u64() != Some(bytes.len() as u64) {
            return Err(CliError::new(
                exit::OTHER,
                "DOCS_SHARE_TARGET_WRITE_FAILED",
                format!("{path} does not match its manifest entry; nothing was mounted"),
            ));
        }
        files.push((path, bytes));
    }
    files.push((
        "SHARE.json".to_owned(),
        str_of(share, "share_json")?.into_bytes(),
    ));
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

fn str_of(v: &Value, key: &str) -> Result<String, CliError> {
    v[key].as_str().map(str::to_owned).ok_or_else(|| {
        CliError::new(
            exit::OTHER,
            "DOCS_ENGINE_INTERNAL",
            format!("response lacks {key}"),
        )
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn report_dry(args: &MountArgs, pack: &Value, trimmed: u64) {
    if args.json {
        println!(
            "{}",
            json!({
                "protocol": "oo.context-mount/v1",
                "dry_run": true,
                "pack_digest": pack["pack_digest"],
                "tokens": pack["tokens"],
                "trimmed": pack["trimmed"],
                "blocked": pack["blocked"],
                "brief": pack["brief_preview"],
                "warnings": [],
            })
        );
    } else {
        println!(
            "dry run: BRIEF {} tokens, trimmed {trimmed}, blocked {}",
            pack["tokens"]["brief"], pack["blocked"]["facts"]
        );
        println!("{}", pack["brief_preview"].as_str().unwrap_or(""));
    }
}

fn report_done(args: &MountArgs, share: &Value, at: &Path) {
    let trimmed = trimmed_total(&share["trimmed"]);
    let blocked = share["blocked"]["facts"].as_u64().unwrap_or(0)
        + share["blocked"]["entities"].as_u64().unwrap_or(0);
    if args.json {
        println!(
            "{}",
            json!({
                "protocol": "oo.context-mount/v1",
                "share_id": share["id"],
                "pack_digest": share["pack_digest"],
                "path": at,
                "tokens": share["tokens"],
                "trimmed": share["trimmed"],
                "blocked": share["blocked"],
                "expires_at": share["expires_at"],
                "warnings": [],
            })
        );
    } else {
        println!(
            "mounted {} (share {}, BRIEF {}/{} tokens, trimmed {trimmed}, blocked {blocked}, expires {})",
            at.display(),
            share["id"].as_str().unwrap_or(""),
            share["tokens"]["brief"],
            share["tokens"]["budget"],
            share["expires_at"].as_str().unwrap_or("")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn mount_arguments_parse() {
        let p = parse_mount_args(&a(&[
            "--scope",
            "Projects/a",
            "--scope=People",
            "--to",
            "/tmp/t1",
            "--for",
            "写周报",
            "--budget",
            "1200",
            "--json",
            "--expires",
            "24h",
            "--seed",
            "k1",
        ]))
        .unwrap();
        assert_eq!(p.scope, ["Projects/a", "People"]);
        assert_eq!(
            (p.to.as_deref(), p.task.as_deref(), p.budget),
            (Some("/tmp/t1"), Some("写周报"), Some(1200))
        );
        assert_eq!(p.expires_secs, Some(86_400));
        assert!(p.json && !p.dry_run);
        assert_eq!(p.seeds, ["k1"]);
    }

    #[test]
    fn bad_mount_arguments_are_refused() {
        for bad in [
            &["--scope", "a"][..],
            &["--to", "/tmp/x", "--budget", "10"],
            &["--to", "/tmp/x", "--budget", "x"],
            &["--to", "/tmp/x", "--role", "admin"],
            &["--to", "/tmp/x", "--expires", "5m"],
            &["--to", "/tmp/x", "--expires", "90d"],
            &["--to", "/tmp/x", "--for", "a", "--for-file", "b"],
            &["--to", "/tmp/x", "--version", "0"],
            &["--to", "/tmp/x", "--bogus"],
            &["--to"],
        ] {
            assert!(parse_mount_args(&a(bad)).is_err(), "{bad:?}");
        }
        assert!(
            parse_mount_args(&a(&["--dry-run"])).is_ok(),
            "a dry run needs no --to"
        );
    }

    #[test]
    fn s6_flags_parse() {
        let p = parse_mount_args(&a(&[
            "--to",
            "/tmp/t",
            "--group",
            "TICKET-7",
            "--refresh-policy",
            "at-checkpoint",
            "--require-complete",
            "--wait",
            "5s",
        ]))
        .unwrap();
        assert_eq!(p.group.as_deref(), Some("TICKET-7"));
        assert_eq!(p.refresh_policy.as_deref(), Some("at-checkpoint"));
        assert!(p.require_complete);
        assert_eq!(p.wait_ms, Some(5000));
        let b = compile_body(&p, "d", None);
        assert_eq!(
            (b["require_complete"].clone(), b["wait_ms"].clone()),
            (json!(true), json!(5000))
        );
        assert!(parse_mount_args(&a(&["--to", "/t", "--wait", "5m"])).is_err());
    }

    #[test]
    fn simple_arguments_parse() {
        let s = parse_simple("status", &a(&["/tmp/x", "--json"])).unwrap();
        assert_eq!(s.target.as_deref(), Some("/tmp/x"));
        assert!(s.json);
        let c = parse_simple("check", &a(&["--dir", "/p/.context", "--since", "4"])).unwrap();
        assert_eq!(
            (c.dir, c.since),
            (Some(PathBuf::from("/p/.context")), Some(4))
        );
        let r = parse_simple("refresh", &a(&["--group", "T1"])).unwrap();
        assert_eq!(r.group.as_deref(), Some("T1"));
        assert!(parse_simple("status", &[]).is_err());
        assert!(parse_simple("refresh", &[]).is_err());
        assert!(parse_simple("list", &a(&["--status", "zzz"])).is_err());
    }

    #[test]
    fn the_gate_closes_on_anything_it_does_not_understand() {
        assert_eq!(check_code(200, &json!({"exit_code": 0})), 0);
        assert_eq!(check_code(200, &json!({"exit_code": 10})), 10);
        assert_eq!(check_code(200, &json!({"exit_code": 20})), 20);
        assert_eq!(check_code(200, &json!({"exit_code": 30})), 30);
        assert_eq!(check_code(410, &json!({})), 30);
        for (status, body) in [
            (200, json!({})),
            (200, json!({"exit_code": 7})),
            (401, json!({})),
            (403, json!({})),
            (500, json!({})),
            (502, Value::Null),
        ] {
            assert_eq!(check_code(status, &body), 40, "{status} {body}");
        }
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("7d"), Some(604_800));
        assert_eq!(parse_duration("24h"), Some(86_400));
        assert_eq!(parse_duration("90m"), Some(5_400));
        assert_eq!(parse_duration("3600"), Some(3_600));
        assert_eq!(parse_duration("0d"), None);
        assert_eq!(parse_duration("x"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn trim_totals() {
        assert_eq!(trimmed_total(&json!({})), 0);
        assert_eq!(
            trimmed_total(&json!({"decisions": {"summary": 0, "title": 0, "removed": 0}})),
            0
        );
        assert_eq!(
            trimmed_total(
                &json!({"a": {"summary": 1, "title": 0, "removed": 2}, "b": {"summary": 0, "title": 3, "removed": 0}})
            ),
            6
        );
    }

    #[test]
    fn compile_body_only_carries_what_was_asked() {
        let args = MountArgs::default();
        let b = compile_body(&args, "d", None);
        assert_eq!(b, json!({"drive_id": "d", "role": "suggester"}));
        let args = MountArgs {
            scope: vec!["P".into()],
            budget: Some(500),
            seeds: vec!["k".into()],
            ..MountArgs::default()
        };
        let b = compile_body(&args, "d", Some("t"));
        assert_eq!(b["scope"], json!(["P"]));
        assert_eq!(
            (b["task"].as_str(), b["budget"].as_u64()),
            (Some("t"), Some(500))
        );
    }
}
