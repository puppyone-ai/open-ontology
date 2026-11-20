//! `oo ontology search|entity|query|sources|schema` (D83): the CLI end of the unified query surface.
//!
//! An Agent calls it with `--endpoint URL --token-file F` (or `OO_ENDPOINT` and `OO_AGENT_TOKEN_FILE`);
//! without them the local `oo serve` and its control token are used. JSON output is one object on one
//! line; human output prints a `[source]` line under each answer.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::client::{CliError, Daemon, exit};
use super::docs_cmd::{Parsed, parse};
use super::federate::pct_encode;
use super::secrets;

pub const HELP: &str = "Usage:\n  oo ontology search <text> [--type <T>] [--limit <n>] [--json]\n  oo ontology entity <urn> [--json]\n  oo ontology query \"<select ...>\" [--max-rows <n>] [--json]\n  oo ontology sources [--json]\n  oo ontology schema [--limit <n>] [--cursor <c>] [--json]   (tables, columns, relationships, document links; read-only)\n  Agent access: --endpoint <url> --token-file <file> (or OO_ENDPOINT and OO_AGENT_TOKEN_FILE); otherwise the local oo serve (--state-dir <dir>).\n  Examples:\n    oo ontology schema                                  tables, columns and foreign keys (--limit is 1..100)\n    oo ontology query \"select count(*) from public.orders\"   one read-only SELECT over the published tables\n    oo ontology search 澄海物流                            people, projects, documents and rows; each hit prints a [source] line\n  Gateway commands such as oo tables and oo query need a saved login, not an Agent token; use the commands above.\n";

pub fn is_subcommand(rest: &[String]) -> bool {
    matches!(
        rest.first().map(String::as_str),
        Some("search" | "entity" | "query" | "sources" | "schema")
    )
}

fn bad(m: impl Into<String>) -> CliError {
    CliError::new(exit::ARGS, "DOCS_ENGINE_BAD_REQUEST", m)
}

/// The token in a private regular file (no group or world access).
pub fn read_token_file(path: &Path) -> Result<String, CliError> {
    let m = std::fs::symlink_metadata(path).map_err(|_| bad("token file is missing"))?;
    if !m.is_file() || m.mode() & 0o077 != 0 {
        return Err(bad("token file must be a private regular file (mode 0600)"));
    }
    let t = std::fs::read_to_string(path).map_err(|_| bad("token file is unreadable"))?;
    Ok(t.trim().to_owned())
}

fn connect(p: &Parsed) -> Result<Daemon, CliError> {
    let endpoint = p
        .opt("--endpoint")
        .map(str::to_owned)
        .or_else(|| std::env::var("OO_ENDPOINT").ok());
    let file = p
        .opt("--token-file")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("OO_AGENT_TOKEN_FILE").map(PathBuf::from));
    match (endpoint, file) {
        (Some(e), Some(f)) => {
            if !(e.starts_with("http://") || e.starts_with("https://")) {
                return Err(bad("--endpoint is http(s)://host:port"));
            }
            Daemon::remote(&e, &read_token_file(&f)?, None)
        }
        (None, None) => {
            let state = p
                .state_dir
                .clone()
                .or_else(|| std::env::var_os("OO_DOCS_STATE_DIR").map(PathBuf::from))
                .or_else(secrets::default_state_dir)
                .ok_or_else(|| bad("no state directory: set --state-dir or HOME"))?;
            Daemon::connect(&state)
        }
        _ => Err(bad("--endpoint and --token-file go together")),
    }
}

/// `[source] ...` line of a provenance object.
pub fn cite(p: &Value) -> String {
    match p["kind"].as_str() {
        Some("file") => {
            let anchor = match &p["anchor"] {
                Value::Null => String::new(),
                a => format!(
                    " #{}",
                    a.as_str().map_or_else(|| a.to_string(), str::to_owned)
                ),
            };
            format!(
                "[source] file {} (version {}){anchor}",
                p["path"].as_str().unwrap_or("?"),
                p["file_version"].as_str().unwrap_or("?")
            )
        }
        Some("table_row") => format!(
            "[source] table {} row {}={} (publication {})",
            p["table"].as_str().unwrap_or("?"),
            p["key_column"].as_str().unwrap_or("pk"),
            p["pk"].as_str().unwrap_or("?"),
            p["publication_id"].as_str().unwrap_or("?")
        ),
        Some("gateway_query") => format!(
            "[source] gateway {} query {} (publication {})",
            p["source"].as_str().unwrap_or("?"),
            p["query_id"].as_str().unwrap_or("?"),
            p["publication_id"].as_str().unwrap_or("?")
        ),
        _ => "[source] unknown".into(),
    }
}

fn human(v: &Value) -> String {
    let mut out = String::new();
    match v["protocol"].as_str().unwrap_or("") {
        "oo.ontology.search/v1" => {
            for it in v["items"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "{} {} {}  {}\n  {}\n",
                    it["side"].as_str().unwrap_or(""),
                    it["type"].as_str().unwrap_or(""),
                    it["name"].as_str().unwrap_or(""),
                    it["urn"].as_str().unwrap_or(""),
                    cite(&it["provenance"])
                ));
                for l in it["links"].as_array().into_iter().flatten() {
                    out.push_str(&format!(
                        "  link -> {}{}\n",
                        l["target"]["urn"].as_str().unwrap_or(""),
                        if l["inferred"] == true {
                            " (inferred)"
                        } else {
                            ""
                        }
                    ));
                }
            }
        }
        "oo.ontology.entity/v1" => {
            out.push_str(&format!(
                "{} {}\n",
                v["urn"].as_str().unwrap_or(""),
                v["name"].as_str().unwrap_or("")
            ));
            if v["side"] == "db" {
                out.push_str(&format!(
                    "  row {}\n  {}\n",
                    v["row"],
                    cite(&v["provenance"])
                ));
            }
            for f in v["facts"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "  {} = {}\n    {}\n",
                    f["predicate"].as_str().unwrap_or(""),
                    f["value"]
                        .as_str()
                        .unwrap_or(&f["object"]["name"].to_string()),
                    cite(&f["provenance"])
                ));
            }
            for l in v["links"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "  link -> {}\n",
                    l["target"]["urn"].as_str().unwrap_or("")
                ));
                if let Some(r) = l.get("row") {
                    out.push_str(&format!(
                        "    row {}\n    {}\n",
                        r["columns"],
                        cite(&r["provenance"])
                    ));
                }
            }
        }
        "oo.ontology.schema/v1" => {
            for t in v["tables"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "{}  {}\n",
                    t["qualified_name"].as_str().unwrap_or(""),
                    t["urn"].as_str().unwrap_or("")
                ));
                for c in t["columns"].as_array().into_iter().flatten() {
                    out.push_str(&format!(
                        "  {} {}\n",
                        c["name"].as_str().unwrap_or(""),
                        c["type"].as_str().unwrap_or("")
                    ));
                }
            }
            for r in v["relationships"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "rel {}: {}.{} -> {}.{} ({})\n",
                    r["id"].as_str().unwrap_or(""),
                    r["from"]["table"].as_str().unwrap_or(""),
                    r["from"]["columns"],
                    r["to"]["table"].as_str().unwrap_or(""),
                    r["to"]["columns"],
                    r["provenance"].as_str().unwrap_or("?")
                ));
            }
            for l in v["doc_links"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "link {} -> {}\n",
                    l["row_urn"].as_str().unwrap_or(""),
                    l["target"]["urn"].as_str().unwrap_or("")
                ));
            }
            if let Some(c) = v["next_cursor"].as_str() {
                out.push_str(&format!("next: --cursor {c}\n"));
            }
            out.push_str(&format!(
                "[source] gateway {} schema",
                v["source"]["profile"].as_str().unwrap_or("?")
            ));
        }
        "oo.ontology.query/v1" => {
            out.push_str(&format!("{}\n", v["columns"]));
            for r in v["rows"].as_array().into_iter().flatten() {
                out.push_str(&format!("{r}\n"));
            }
            out.push_str(&format!("{}\n", cite(&v["provenance"])));
        }
        _ => {
            for s in v["items"].as_array().into_iter().flatten() {
                out.push_str(&format!("{s}\n"));
            }
        }
    }
    out.trim_end().to_owned()
}

/// Entry for `oo ontology <search|entity|query|sources> ...`; returns the exit code.
pub fn run(rest: &[String]) -> u8 {
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return exit::OK;
    }
    let p = match parse(
        &rest[1..],
        &[
            "--type",
            "--limit",
            "--max-rows",
            "--cursor",
            "--endpoint",
            "--token-file",
        ],
        &[],
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("oo ontology: {e}");
            return exit::ARGS;
        }
    };
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return exit::OTHER;
    };
    let res: Result<Value, CliError> = rt.block_on(async {
        let d = connect(&p)?;
        let arg = || {
            p.pos
                .first()
                .ok_or_else(|| bad("missing argument; see `oo ontology --help`"))
        };
        match rest[0].as_str() {
            "search" => {
                let mut path = format!("/v1/ontology/search?q={}", pct_encode(arg()?));
                if let Some(t) = p.opt("--type") {
                    path.push_str(&format!("&type={}", pct_encode(t)));
                }
                if let Some(l) = p.opt("--limit") {
                    path.push_str(&format!("&limit={}", pct_encode(l)));
                }
                d.get_json(&path).await
            }
            "entity" => {
                d.get_json(&format!("/v1/ontology/entities/{}", pct_encode(arg()?)))
                    .await
            }
            "sources" => d.get_json("/v1/ontology/sources").await,
            "schema" => {
                let mut path = String::from("/v1/ontology/schema");
                let mut sep = '?';
                for (flag, key) in [("--limit", "limit"), ("--cursor", "cursor")] {
                    if let Some(v) = p.opt(flag) {
                        path.push_str(&format!("{sep}{key}={}", pct_encode(v)));
                        sep = '&';
                    }
                }
                d.get_json(&path).await
            }
            _ => {
                let mut body = json!({"sql": arg()?});
                if let Some(n) = p.opt("--max-rows") {
                    body["max_rows"] = json!(
                        n.parse::<u64>()
                            .map_err(|_| bad("--max-rows is a number"))?
                    );
                }
                d.post_json("/v1/ontology/query", &body).await
            }
        }
    });
    match res {
        Ok(v) => {
            if p.json {
                println!("{v}");
            } else {
                println!("{}", human(&v));
            }
            exit::OK
        }
        Err(e) => {
            eprintln!(
                "{}",
                json!({"protocol": "oo.docs-error/v1", "error": {"code": e.code, "message": e.message}})
            );
            e.exit
        }
    }
}

/// `oo mcp --stdio --endpoint URL --token-file F`: the same unified tools for any MCP agent.
pub fn mcp(rest: &[String]) -> Result<crate::args::Invocation, crate::outcome::Failure> {
    let usage = |m: &str| crate::outcome::Failure::usage(m);
    let p = parse(rest, &["--endpoint", "--token-file"], &["--stdio"])
        .map_err(|_| usage("oo mcp --stdio --endpoint <url> --token-file <file>"))?;
    let (Some(e), Some(f)) = (p.opt("--endpoint"), p.opt("--token-file")) else {
        return Err(usage("oo mcp --stdio --endpoint <url> --token-file <file>"));
    };
    if !p.has("--stdio") || !Path::new(f).is_absolute() {
        return Err(usage("--stdio and an absolute --token-file are required"));
    }
    read_token_file(Path::new(f)).map_err(|e| usage(&e.message))?;
    let exe = std::env::current_exe().map_err(|_| crate::outcome::Failure::local("MCP_INSTALL"))?;
    use std::os::unix::process::CommandExt;
    let _error = std::process::Command::new(exe.with_file_name("mcp-gateway"))
        .env("OO_MCP_ONTOLOGY_URL", e)
        .env("OO_MCP_ONTOLOGY_TOKEN_FILE", f)
        .env_remove("OO_MCP_PROFILE")
        .env_remove("OO_MCP_GATEWAY_URL")
        .env_remove("OO_MCP_TOKEN_FILE")
        .exec();
    Err(crate::outcome::Failure::local("MCP_INSTALL"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citations_name_the_file_or_the_row() {
        let f = json!({"kind":"file","path":"a/b.md","file_version":"v1","anchor":"h1"});
        assert!(cite(&f).starts_with("[source] file a/b.md"));
        let r = json!({"kind":"table_row","table":"s.t","key_column":"id","pk":"7","publication_id":"p1"});
        assert_eq!(cite(&r), "[source] table s.t row id=7 (publication p1)");
    }

    #[test]
    fn token_file_must_be_private() {
        use std::os::unix::fs::PermissionsExt;
        let f = std::env::temp_dir().join(format!("oo-ont-tok-{}", std::process::id()));
        std::fs::write(&f, "ooa_t\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_token_file(&f).is_err());
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_token_file(&f).unwrap(), "ooa_t");
        let _ = std::fs::remove_file(&f);
    }
}
