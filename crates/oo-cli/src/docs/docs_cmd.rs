//! `oo docs ...`: the Desktop's whole operating surface as thin clients of the
//! control API through `oo serve` (D80, 30 §7.2).
//!
//! ```text
//! oo docs drive add|ls|show|rm|sync      oo docs files ls|show|retry
//! oo docs review ls|decide|accept-source oo docs entity search|show|correct|merge|split
//! oo docs share ls|show|revoke|refresh   oo docs usage | health
//! ```
//!
//! Every command takes `--json` (one JSON document on stdout) and
//! `--state-dir`. Human output is one line per item. Exit codes are those of
//! `oo context` (10 §7.9): 2 for arguments, 3 when `oo serve` is not running.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::client::{CliError, Daemon, exit};

pub const HELP: &str = "Usage:\n  oo docs drive add --name <n> --root <dir> | ls | show <drive> | rm <drive> | sync <drive> [--deep] [--path <p>]...\n  oo docs files ls --drive <drive> [--state <s>] | show <file-id> | retry <file-id>...\n  oo docs review ls [--kind <k>] [--status <s>] | decide accept|reject <item-id>... | accept-source <file-id> [--reject]\n  oo docs entity search <text> [--type <T>] | show <key> [--history] | correct <fact-id> (--mark-wrong | --set <text>) --entity <key> | merge --into <key> <from-key>... | split <key> --groups <json>\n  oo docs share ls | show <share-id> | revoke <share-id|dir> | refresh <share-id|dir>\n  oo docs usage | health\n  (all take --json and --state-dir <dir>)\n";

/// Common flags and the leftover positional arguments.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub json: bool,
    pub state_dir: Option<PathBuf>,
    /// `--flag value` pairs in order (a flag may repeat).
    pub opts: Vec<(String, String)>,
    /// Flags without a value.
    pub switches: Vec<String>,
    pub pos: Vec<String>,
}

impl Parsed {
    pub fn opt(&self, name: &str) -> Option<&str> {
        self.opts
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn all(&self, name: &str) -> Vec<&str> {
        self.opts
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn has(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }
}

/// Split `args` into flags that take a value (`valued`), switches
/// (`switch`) and positionals; anything else with `--` is an error.
pub fn parse(args: &[String], valued: &[&str], switch: &[&str]) -> Result<Parsed, String> {
    let mut p = Parsed::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_owned())),
            _ => (a.as_str(), None),
        };
        match flag {
            "--json" => p.json = true,
            "--state-dir" => {
                let v = inline
                    .or_else(|| it.next().cloned())
                    .ok_or("--state-dir needs a value")?;
                p.state_dir = Some(PathBuf::from(v));
            }
            f if valued.contains(&f) => {
                let v = inline
                    .or_else(|| it.next().cloned())
                    .ok_or_else(|| format!("{f} needs a value"))?;
                p.opts.push((f.to_owned(), v));
            }
            f if switch.contains(&f) => p.switches.push(f.to_owned()),
            f if f.starts_with("--") => return Err(format!("unknown option {f}")),
            _ => p.pos.push(a.clone()),
        }
    }
    Ok(p)
}

fn bad(msg: impl Into<String>) -> CliError {
    CliError::new(exit::ARGS, "DOCS_ENGINE_BAD_REQUEST", msg)
}

fn rev_header(rev: i64) -> String {
    format!("\"r{rev}\"")
}

fn q(s: &str) -> String {
    super::client::encode_path(s)
        .replace('/', "%2F")
        .replace(':', "%3A")
}

fn out(p: &Parsed, protocol: &str, mut v: Value, human: String) {
    if p.json {
        if v.is_object() {
            v["protocol"] = json!(protocol);
            println!("{v}");
        } else {
            println!("{}", json!({"protocol": protocol, "result": v}));
        }
    } else if !human.is_empty() {
        println!("{human}");
    }
}

/// A drive by id or name.
async fn drive_id(d: &Daemon, want: &str) -> Result<String, CliError> {
    let list = d.get_json("/v1/docs/drives?limit=200").await?;
    list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|i| {
            i["id"] == want
                || i["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(want))
        })
        .and_then(|i| i["id"].as_str().map(str::to_owned))
        .ok_or_else(|| {
            CliError::new(
                exit::OTHER,
                "DOCS_SOURCE_DRIVE_NOT_FOUND",
                format!("no drive {want}"),
            )
        })
}

fn state_of(p: &Parsed) -> Result<PathBuf, CliError> {
    p.state_dir
        .clone()
        .or_else(|| std::env::var_os("OO_DOCS_STATE_DIR").map(PathBuf::from))
        .or_else(super::secrets::default_state_dir)
        .ok_or_else(|| bad("no state directory: set --state-dir or HOME"))
}

/// Entry for `oo docs <rest...>`; returns the exit code.
pub fn run(rest: &[String]) -> u8 {
    let (Some(group), tail) = (rest.first(), rest.get(1..).unwrap_or(&[])) else {
        eprint!("{HELP}");
        return exit::ARGS;
    };
    let Some((sub, args)) = tail
        .split_first()
        .map(|(s, a)| (s.as_str(), a))
        .or_else(|| matches!(group.as_str(), "usage" | "health").then_some(("", tail)))
    else {
        eprint!("{HELP}");
        return exit::ARGS;
    };
    let valued = [
        "--name",
        "--root",
        "--backend",
        "--path",
        "--drive",
        "--state",
        "--kind",
        "--status",
        "--limit",
        "--type",
        "--set",
        "--reason",
        "--entity",
        "--into",
        "--groups",
        "--revision",
        "--confirm-mass-delete",
        "--kinds",
        "--q",
    ];
    let switches = [
        "--deep",
        "--safe",
        "--history",
        "--mark-wrong",
        "--reject",
        "--model",
        "--yes",
    ];
    let p = match parse(args, &valued, &switches) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("oo docs {group}: {e}");
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
    // `share` aliases `oo context` where they overlap.
    if group == "share" {
        let mut forwarded: Vec<String> = Vec::new();
        let (verb, pos): (&str, Vec<String>) = match sub {
            "ls" => ("list", p.pos.clone()),
            "show" => ("status", p.pos.clone()),
            "revoke" | "refresh" => (sub, p.pos.clone()),
            other => {
                eprintln!("oo docs share: unknown subcommand `{other}`");
                return exit::ARGS;
            }
        };
        forwarded.push(verb.to_owned());
        forwarded.extend(pos);
        if p.json {
            forwarded.push("--json".into());
        }
        if let Some(s) = &p.state_dir {
            forwarded.push("--state-dir".into());
            forwarded.push(s.display().to_string());
        }
        if let Some(s) = p.opt("--status") {
            forwarded.push("--status".into());
            forwarded.push(s.to_owned());
        }
        return super::context::run(&forwarded);
    }
    let result = rt.block_on(async {
        let d = Daemon::connect(&state_of(&p)?)?;
        match (group.as_str(), sub) {
            ("drive", "add") => drive_add(&d, &p).await,
            ("drive", "ls") => drive_ls(&d, &p).await,
            ("drive", "show") => drive_show(&d, &p).await,
            ("drive", "rm") => drive_rm(&d, &p).await,
            ("drive", "sync") => drive_sync(&d, &p).await,
            ("files", "ls") => files_ls(&d, &p).await,
            ("files", "show") => files_show(&d, &p).await,
            ("files", "retry") => files_retry(&d, &p).await,
            ("review", "ls") => review_ls(&d, &p).await,
            ("review", "decide") => review_decide(&d, &p).await,
            ("review", "accept-source") => review_source(&d, &p).await,
            ("entity", "search") => entity_search(&d, &p).await,
            ("entity", "show") => entity_show(&d, &p).await,
            ("entity", "correct") => entity_correct(&d, &p).await,
            ("entity", "merge") => entity_merge(&d, &p).await,
            ("entity", "split") => entity_split(&d, &p).await,
            ("usage", _) => simple_get(&d, &p, "/v1/docs/usage", "oo.docs-usage/v1").await,
            ("health", _) => simple_get(&d, &p, "/v1/docs/health", "oo.docs-health/v1").await,
            (g, s) => Err(bad(format!("unknown command `{g} {s}`"))),
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
                eprintln!("oo docs {group}: {} ({})", e.message, e.code);
            }
            e.exit
        }
    }
}

fn one<'a>(p: &'a Parsed, what: &str) -> Result<&'a str, CliError> {
    p.pos
        .first()
        .map(String::as_str)
        .ok_or_else(|| bad(format!("expected {what}")))
}

async fn simple_get(d: &Daemon, p: &Parsed, path: &str, proto: &str) -> Result<(), CliError> {
    let v = d.get_json(path).await?;
    out(
        p,
        proto,
        v.clone(),
        serde_json::to_string_pretty(&v).unwrap_or_default(),
    );
    Ok(())
}

// ----------------------------------------------------------------- drive ---

fn drive_line(v: &Value) -> String {
    let f = &v["freshness"];
    format!(
        "{}  {}  {}  files {}  {}  rev {}",
        v["id"].as_str().unwrap_or(""),
        v["name"].as_str().unwrap_or(""),
        v["status"].as_str().unwrap_or(""),
        v["files_total"],
        f["state"].as_str().unwrap_or("-"),
        v["drive_rev"]
    )
}

async fn drive_add(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let name = p.opt("--name").ok_or_else(|| bad("--name is required"))?;
    let root = p.opt("--root").ok_or_else(|| bad("--root is required"))?;
    let root = std::fs::canonicalize(root)
        .map_err(|_| bad("--root does not exist"))?
        .display()
        .to_string();
    let mut body = json!({"name": name, "backend": p.opt("--backend").unwrap_or("fs"),
                          "config": {"root": root}});
    if p.has("--model") {
        body["model_enabled"] = json!(true);
    }
    let v = d.post_json("/v1/docs/drives", &body).await?;
    out(
        p,
        "oo.docs-drive/v1",
        v.clone(),
        format!("added {}", drive_line(&v)),
    );
    Ok(())
}

async fn drive_ls(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let v = d.get_json("/v1/docs/drives?limit=200").await?;
    let items = v["items"].as_array().cloned().unwrap_or_default();
    let human = if items.is_empty() {
        "no drives".to_owned()
    } else {
        items.iter().map(drive_line).collect::<Vec<_>>().join("\n")
    };
    out(p, "oo.docs-drives/v1", json!({"drives": items}), human);
    Ok(())
}

async fn drive_show(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let id = drive_id(d, one(p, "<drive>")?).await?;
    let v = d.get_json(&format!("/v1/docs/drives/{id}")).await?;
    out(p, "oo.docs-drive/v1", v.clone(), drive_line(&v));
    Ok(())
}

async fn drive_rm(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let id = drive_id(d, one(p, "<drive>")?).await?;
    let v = d.get_json(&format!("/v1/docs/drives/{id}")).await?;
    let rev = v["revision"].as_i64().unwrap_or(1);
    let r = d
        .delete(&format!("/v1/docs/drives/{id}"), Some(&rev_header(rev)))
        .await?;
    out(p, "oo.docs-drive/v1", r.clone(), format!("deleting {id}"));
    Ok(())
}

async fn drive_sync(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let id = drive_id(d, one(p, "<drive>")?).await?;
    let mut body = json!({});
    if p.has("--deep") {
        body["deep"] = json!(true);
    }
    let paths = p.all("--path");
    if !paths.is_empty() {
        body["paths"] = json!(paths);
    }
    if let Some(g) = p.opt("--confirm-mass-delete") {
        body["confirm_mass_delete"] = json!(
            g.parse::<i64>()
                .map_err(|_| bad("--confirm-mass-delete is a generation"))?
        );
    }
    let v = d
        .post_json(&format!("/v1/docs/drives/{id}:sync"), &body)
        .await?;
    let human = format!(
        "sync {}: generation {}, +{} ~{} -{}, drive_rev {}",
        v["status"].as_str().unwrap_or(""),
        v["generation"],
        v["added"],
        v["changed"],
        v["removed"],
        v["drive_rev"]
    );
    out(p, "oo.docs-sync/v1", v, human);
    Ok(())
}

// ----------------------------------------------------------------- files ---

async fn files_ls(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let drive = drive_id(
        d,
        p.opt("--drive").ok_or_else(|| bad("--drive is required"))?,
    )
    .await?;
    let mut path = format!(
        "/v1/docs/drives/{drive}/files?limit={}",
        p.opt("--limit").unwrap_or("200")
    );
    if let Some(s) = p.opt("--state") {
        path.push_str(&format!("&state={}", q(s)));
    }
    let v = d.get_json(&path).await?;
    let items = v["items"].as_array().cloned().unwrap_or_default();
    let human = items
        .iter()
        .map(|f| {
            format!(
                "{}  {:<14} {}",
                f["id"].as_str().unwrap_or(""),
                f["state"].as_str().unwrap_or(""),
                f["path"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    out(
        p,
        "oo.docs-files/v1",
        json!({"files": items, "next_cursor": v["next_cursor"]}),
        human,
    );
    Ok(())
}

async fn files_show(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let id = one(p, "<file-id>")?;
    let v = d
        .get_json(&format!("/v1/docs/files/{id}?include=history"))
        .await?;
    let human = format!(
        "{}  {}  {}{}",
        v["id"].as_str().unwrap_or(""),
        v["state"].as_str().unwrap_or(""),
        v["path"].as_str().unwrap_or(""),
        v["state_code"]
            .as_str()
            .map(|c| format!("  ({c})"))
            .unwrap_or_default()
    );
    out(p, "oo.docs-file/v1", v, human);
    Ok(())
}

async fn files_retry(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    if p.pos.is_empty() {
        return Err(bad("expected <file-id>..."));
    }
    let mode = if p.has("--safe") { "safe" } else { "normal" };
    let v = d
        .post_json(
            "/v1/docs/files:retry",
            &json!({"file_ids": p.pos, "mode": mode}),
        )
        .await?;
    let human = v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            format!(
                "{}  {}",
                r["id"].as_str().unwrap_or(""),
                r["status"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    out(p, "oo.docs-retry/v1", v, human);
    Ok(())
}

// ---------------------------------------------------------------- review ---

async fn review_ls(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let mut path = String::from("/v1/docs/review?limit=200");
    for (flag, key) in [
        ("--kind", "kind"),
        ("--status", "status"),
        ("--drive", "drive_id"),
    ] {
        if let Some(v) = p.opt(flag) {
            let v = if key == "drive_id" {
                drive_id(d, v).await?
            } else {
                v.to_owned()
            };
            path.push_str(&format!("&{key}={}", q(&v)));
        }
    }
    let v = d.get_json(&path).await?;
    let items = v["items"].as_array().cloned().unwrap_or_default();
    let human = if items.is_empty() {
        "the queue is empty".to_owned()
    } else {
        items
            .iter()
            .map(|i| {
                format!(
                    "{}  r{}  {:<22} {:<9} {}",
                    i["id"].as_str().unwrap_or(""),
                    i["revision"],
                    i["kind"].as_str().unwrap_or(""),
                    i["status"].as_str().unwrap_or(""),
                    i["source"]["path"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    out(
        p,
        "oo.docs-review/v1",
        json!({"items": items, "groups": v["groups"]}),
        human,
    );
    Ok(())
}

async fn review_decide(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let (decision, ids) = match p.pos.split_first() {
        Some((dec, ids)) if matches!(dec.as_str(), "accept" | "reject") && !ids.is_empty() => {
            (dec.as_str(), ids)
        }
        _ => return Err(bad("expected accept|reject <item-id>...")),
    };
    let queue = d
        .get_json("/v1/docs/review?limit=1000&status=pending")
        .await?;
    let items: Vec<Value> = ids
        .iter()
        .map(|id| {
            let rev = queue["items"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|i| i["id"] == id.as_str())
                .and_then(|i| i["revision"].as_i64())
                .ok_or_else(|| bad(format!("{id} is not a pending item")))?;
            Ok(json!({"id": id, "revision": rev, "decision": decision}))
        })
        .collect::<Result<_, CliError>>()?;
    let v = d
        .post_json("/v1/docs/review:decide", &json!({"items": items}))
        .await?;
    let human = format!(
        "accepted {}, rejected {}{}",
        v["accepted"],
        v["rejected"],
        if v["publish_pending"] == true {
            "; a new version is being published"
        } else {
            ""
        }
    );
    out(p, "oo.docs-decide/v1", v, human);
    Ok(())
}

async fn review_source(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let file = one(p, "<file-id>")?;
    let decision = if p.has("--reject") {
        "reject"
    } else {
        "accept"
    };
    let mut by = json!({"file_id": file, "decision": decision});
    if let Some(k) = p.opt("--kinds") {
        by["kinds"] = json!(k.split(',').collect::<Vec<_>>());
    }
    let v = d
        .post_json("/v1/docs/review:decide", &json!({"by_source": by}))
        .await?;
    let human = format!(
        "{decision}ed {} items of {file}",
        if decision == "accept" {
            &v["accepted"]
        } else {
            &v["rejected"]
        }
    );
    out(p, "oo.docs-decide/v1", v, human);
    Ok(())
}

// ---------------------------------------------------------------- entity ---

async fn entity_search(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let text = one(p, "<text>")?;
    let mut path = format!("/v1/docs/entities?q={}&limit=50", q(text));
    if let Some(t) = p.opt("--type") {
        path.push_str(&format!("&type={}", q(t)));
    }
    let v = d.get_json(&path).await?;
    let items = v["items"].as_array().cloned().unwrap_or_default();
    let human = items
        .iter()
        .map(|e| {
            format!(
                "{}  {}  {}",
                e["key"].as_str().unwrap_or(""),
                e["type"].as_str().unwrap_or(""),
                e["name"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    out(p, "oo.docs-entities/v1", json!({"entities": items}), human);
    Ok(())
}

async fn entity_show(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let key = one(p, "<entity-key>")?;
    let mut path = format!("/v1/docs/entities/{}", q(key));
    if p.has("--history") {
        path.push_str("?include=history");
    }
    let v = d.get_json(&path).await?;
    let mut lines = vec![format!(
        "{}  {}  {} (r{})",
        v["key"].as_str().unwrap_or(""),
        v["type"].as_str().unwrap_or(""),
        v["name"].as_str().unwrap_or(""),
        v["revision"]
    )];
    for a in v["aliases"].as_array().into_iter().flatten() {
        lines.push(format!("  alias {}", a["alias"].as_str().unwrap_or("")));
    }
    for f in v["facts"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(v["history"].as_array().into_iter().flatten())
    {
        lines.push(format!(
            "  {} {}  {} {} (r{})",
            f["id"].as_str().unwrap_or(""),
            f["status"].as_str().unwrap_or(""),
            f["predicate"].as_str().unwrap_or(""),
            f["object"]["name"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| f["value"].to_string()),
            f["revision"]
        ));
    }
    out(p, "oo.docs-entity/v1", v, lines.join("\n"));
    Ok(())
}

async fn entity_revision(d: &Daemon, key: &str) -> Result<i64, CliError> {
    let v = d.get_json(&format!("/v1/docs/entities/{}", q(key))).await?;
    v["revision"]
        .as_i64()
        .ok_or_else(|| bad("the entity has no revision"))
}

async fn entity_correct(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let fact = one(p, "<fact-id>")?;
    let body = match (p.has("--mark-wrong"), p.opt("--set")) {
        (true, None) => {
            let mut b = json!({"action": "mark_wrong"});
            if let Some(r) = p.opt("--reason") {
                b["reason"] = json!(r);
            }
            b
        }
        (false, Some(v)) => json!({"action": "set_value", "value": {"text": v}}),
        _ => return Err(bad("pass exactly one of --mark-wrong or --set <text>")),
    };
    let rev = match (p.opt("--revision"), p.opt("--entity")) {
        (Some(r), _) => r
            .parse::<i64>()
            .map_err(|_| bad("--revision is a number"))?,
        (None, Some(e)) => {
            let v = d
                .get_json(&format!("/v1/docs/entities/{}?include=history", q(e)))
                .await?;
            v["facts"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(v["history"].as_array().into_iter().flatten())
                .find(|f| f["id"] == fact)
                .and_then(|f| f["revision"].as_i64())
                .ok_or_else(|| bad("the fact is not on that entity"))?
        }
        (None, None) => {
            return Err(bad(
                "pass --entity <key> or --revision <n> (the If-Match revision of the fact)",
            ));
        }
    };
    let v = d
        .post_json_if(
            &format!("/v1/docs/facts/{fact}:correct"),
            &body,
            &rev_header(rev),
        )
        .await?;
    let human = format!(
        "{} -> {} (origin {})",
        fact,
        v["status"].as_str().unwrap_or(""),
        v["origin"].as_str().unwrap_or("")
    );
    out(p, "oo.docs-fact/v1", v, human);
    Ok(())
}

async fn entity_merge(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let into = p
        .opt("--into")
        .ok_or_else(|| bad("--into <key> is required"))?;
    if p.pos.is_empty() {
        return Err(bad("expected <from-key>..."));
    }
    let rev = entity_revision(d, into).await?;
    let v = d
        .post_json_if(
            "/v1/docs/entities:merge",
            &json!({"into": into, "from": p.pos}),
            &rev_header(rev),
        )
        .await?;
    let human = format!(
        "merged {} into {}",
        p.pos.join(", "),
        v["key"].as_str().unwrap_or(into)
    );
    out(p, "oo.docs-entity/v1", v, human);
    Ok(())
}

async fn entity_split(d: &Daemon, p: &Parsed) -> Result<(), CliError> {
    let key = one(p, "<entity-key>")?;
    let groups: Value = serde_json::from_str(
        p.opt("--groups")
            .ok_or_else(|| bad("--groups <json> is required"))?,
    )
    .map_err(|_| bad("--groups is JSON like [{\"aliases\":[\"a\"]},{\"aliases\":[\"b\"]}]"))?;
    let rev = entity_revision(d, key).await?;
    let v = d
        .post_json_if(
            &format!("/v1/docs/entities/{}:split", q(key)),
            &json!({"groups": groups}),
            &rev_header(rev),
        )
        .await?;
    out(
        p,
        "oo.docs-split/v1",
        v.clone(),
        format!(
            "split {key} into {} entities",
            v["entities"].as_array().map_or(0, Vec::len)
        ),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn flags_switches_and_positionals_split() {
        let p = parse(
            &a(&[
                "--json", "--name", "资料", "x", "--deep", "--path=a", "--path", "b", "y",
            ]),
            &["--name", "--path"],
            &["--deep"],
        )
        .unwrap();
        assert!(p.json && p.has("--deep"));
        assert_eq!(p.opt("--name"), Some("资料"));
        assert_eq!(p.all("--path"), ["a", "b"]);
        assert_eq!(p.pos, ["x", "y"]);
        assert!(parse(&a(&["--bogus"]), &[], &[]).is_err());
        assert!(parse(&a(&["--name"]), &["--name"], &[]).is_err());
    }

    #[test]
    fn revisions_are_quoted_for_if_match() {
        assert_eq!(rev_header(3), "\"r3\"");
    }

    #[test]
    fn query_values_are_escaped() {
        assert_eq!(q("a b:c/d"), "a%20b%3Ac%2Fd");
    }
}
