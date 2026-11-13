//! `oo ontology graph`: the screen-4 graph and confirmation without a TTY
//! (`--format ascii|text|mermaid`, `--confirm-summary HASH --reason TEXT`),
//! or the interactive confirm screen on a TTY.
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::tui::adjudication::{self, Adjudication, Overrides};
use crate::tui::graph;

pub const HELP: &str = "\
oo ontology graph [--adjudication adjudication.json] [--format ascii|text|mermaid]
                  [--width N] [--focus ENTITY] [--overrides overrides.json] [--json]
oo ontology graph --confirm-summary SHA256 --reason TEXT [--overrides overrides.json] [--out PATH]
Renders the adjudicated ontology (oo.adjudication/v1). On a TTY without --format
it opens the graph confirm screen: override decisions, then confirm once.
--confirm-summary must equal the summary recomputed after overrides; it writes
oo.publish-confirm/v1 (default publish-confirm.json beside the adjudication).
Existing files are never overwritten.";

fn resolve_focus<'a>(adj: &'a Adjudication, wanted: &str) -> Option<&'a str> {
    adj.graph
        .entities
        .iter()
        .find(|e| {
            e.id == wanted
                || e.label == wanted
                || e.table == wanted
                || e.table.rsplit('.').next() == Some(wanted)
        })
        .map(|e| e.id.as_str())
}

pub fn run(args: &[String]) -> Result<String, &'static str> {
    if args.iter().any(|s| s == "--help" || s == "-h") {
        return Ok(HELP.into());
    }
    let mut opts: BTreeMap<&str, String> = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let (key, inline) = match args[index].split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v.to_owned())),
            _ => (args[index].as_str(), None),
        };
        let value = match key {
            "--json" | "--yes" | "--no-anim" => String::new(),
            "--adjudication" | "--format" | "--width" | "--focus" | "--overrides"
            | "--confirm-summary" | "--reason" | "--out" | "--config" => match inline {
                Some(v) if !v.is_empty() => v,
                Some(_) => return Err("ADMIN_ARGUMENT"),
                None => {
                    index += 1;
                    args.get(index)
                        .filter(|s| !s.is_empty() && !s.starts_with("--"))
                        .cloned()
                        .ok_or("ADMIN_ARGUMENT")?
                }
            },
            _ => return Err("ADMIN_ARGUMENT"),
        };
        let key = match key {
            "--json" => "--json",
            "--yes" => "--yes",
            "--no-anim" => "--no-anim",
            "--adjudication" => "--adjudication",
            "--format" => "--format",
            "--width" => "--width",
            "--focus" => "--focus",
            "--overrides" => "--overrides",
            "--confirm-summary" => "--confirm-summary",
            "--reason" => "--reason",
            "--out" => "--out",
            _ => "--config",
        };
        if opts.insert(key, value).is_some() {
            return Err("ADMIN_ARGUMENT");
        }
        index += 1;
    }
    let path = PathBuf::from(
        opts.get("--adjudication")
            .map_or("adjudication.json", String::as_str),
    );
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let adj = adjudication::load(&path)?;
    let overrides: Option<Overrides> = match opts.get("--overrides") {
        Some(p) => Some(
            serde_json::from_value(adjudication::read_json(Path::new(p))?)
                .map_err(|_| "OVERRIDE_SHAPE")?,
        ),
        None => None,
    };
    let effective = match &overrides {
        Some(o) => adjudication::apply(&adj, o)?,
        None => adj.clone(),
    };
    let json_out = opts.contains_key("--json");
    if let Some(given) = opts.get("--confirm-summary") {
        if opts.contains_key("--format") {
            return Err("ADMIN_ARGUMENT");
        }
        let reason = opts.get("--reason").ok_or("CONFIRM_REASON_REQUIRED")?;
        let doc = adjudication::confirm(
            &adj,
            overrides.as_ref(),
            given,
            reason,
            &adjudication::now_rfc3339(),
        )?;
        let out = opts
            .get("--out")
            .map_or_else(|| dir.join("publish-confirm.json"), PathBuf::from);
        adjudication::write_new(&out, &doc)?;
        return Ok(if json_out {
            json!({"protocol":"oo.publish-confirm-report/v1","path":out,
                "summary_sha256":doc["summary_sha256"],"overrides_sha256":doc["overrides_sha256"]})
            .to_string()
        } else {
            format!(
                "Confirmation written to {} (summary sha256:{}).",
                out.display(),
                effective.summary_sha256
            )
        });
    }
    if opts.contains_key("--reason") || opts.contains_key("--out") {
        return Err("ADMIN_ARGUMENT");
    }
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if tty && !json_out && !opts.contains_key("--format") {
        return interactive(adj, &dir, opts.contains_key("--no-anim"));
    }
    let width = match opts.get("--width") {
        Some(w) => w
            .parse::<usize>()
            .ok()
            .filter(|w| (40..=400).contains(w))
            .ok_or("ADMIN_ARGUMENT")?,
        None => 120,
    };
    let focus = match opts.get("--focus") {
        Some(f) => Some(resolve_focus(&effective, f).ok_or("GRAPH_FOCUS_UNKNOWN")?),
        None => None,
    };
    let format = opts.get("--format").map_or("ascii", String::as_str);
    let text = match format {
        "ascii" => graph::ascii(&effective, width, focus),
        "text" => graph::text(&effective),
        "mermaid" => graph::mermaid(&effective),
        _ => return Err("ADMIN_ARGUMENT"),
    };
    Ok(if json_out {
        json!({"protocol":"oo.ontology-graph/v1","format":format,
            "summary_sha256":effective.summary_sha256,"counts":adjudication::counts(&effective),
            "text":text})
        .to_string()
    } else {
        text.trim_end().to_owned()
    })
}

fn interactive(adj: Adjudication, dir: &Path, no_anim: bool) -> Result<String, &'static str> {
    use crate::tui::{app, theme::Theme};
    let mut term = app::Term::open(Theme::from_env(no_anim))?;
    let result = app::confirm(&mut term, adj, &adjudication::now_rfc3339())?;
    drop(term);
    let Some((doc, overrides)) = result else {
        return Err("TUI_CANCELLED");
    };
    let mut written = Vec::new();
    if !overrides.items.is_empty() {
        let path = dir.join("overrides.json");
        adjudication::write_new(&path, &json!(overrides))?;
        written.push(path);
    }
    let path = dir.join("publish-confirm.json");
    adjudication::write_new(&path, &doc)?;
    written.push(path);
    Ok(format!(
        "Confirmed summary sha256:{}. Wrote {}.",
        doc["summary_sha256"].as_str().unwrap_or_default(),
        written
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}
