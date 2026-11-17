//! `oo context mount` (10 §7.9, L2 subset): compile a pack for a scope, create
//! the share and write `<dir>/.context` atomically and read-only.
//!
//! Flags: `--drive`, `--scope` (repeatable), `--to`, `--for` / `--for-file`,
//! `--seed` (repeatable), `--budget`, `--role`, `--expires`, `--version`,
//! `--strict-budget`, `--dry-run`, `--json`, `--state-dir`. `list`, `status`,
//! `refresh` and `revoke` come with the share lifecycle. The control token and
//! the refs credential are never printed.

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
                if !matches!(r.as_str(), "viewer" | "suggester") {
                    return Err("--role must be viewer or suggester".into());
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
    match rest.first().map(String::as_str) {
        Some("mount") => {}
        Some(other) => {
            eprintln!("oo context: `{other}` is not available yet; supported: mount");
            return exit::ARGS;
        }
        None => {
            eprintln!("oo context: expected a subcommand: mount");
            return exit::ARGS;
        }
    }
    let args = match parse_mount_args(&rest[1..]) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("oo context mount: {e}");
            return exit::ARGS;
        }
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return exit::OTHER,
    };
    match rt.block_on(mount(&args)) {
        Ok(()) => exit::OK,
        Err(e) => fail(&e, args.json),
    }
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
    let share = daemon.post_json("/v1/docs/shares", &body).await?;
    let (sid, pid) = (str_of(&share, "id")?, str_of(&share, "pack_id")?);

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for f in share["files"].as_array().cloned().unwrap_or_default() {
        let path = str_of(&f, "path")?;
        let bytes = daemon
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
    let share_json = str_of(&share, "share_json")?;
    files.push(("SHARE.json".to_owned(), share_json.into_bytes()));
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mounted = mounts::install(&target, &sid, &files).map_err(|e| target_error(&e))?;
    report_done(args, &share, &mounted);
    Ok(())
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
            &["--to", "/tmp/x", "--role", "editor"],
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
