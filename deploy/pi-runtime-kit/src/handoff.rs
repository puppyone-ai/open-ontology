//! Machine-readable handoff for the agent-entry owner.
//! Paths are the container paths seen when the kit volume is mounted at
//! `/opt/pi-runtime-kit`. The mountpoint is the local Docker volume path.

use std::collections::BTreeMap;

use crate::json::{self, Json};
use crate::manifest::read_facts;

pub fn build(facts_text: &str, cjk_text: &str, probe_text: &str, volume: &str, mountpoint: &str) -> Result<String, String> {
    let facts = read_facts(facts_text)?;
    require(&facts, "source_commit")?;
    require(&facts, "pi_commit")?;
    require(&facts, "pi_version")?;
    require(&facts, "node_version")?;
    require(&facts, "image")?;
    require(&facts, "profile")?;
    require(&facts, "bash_version")?;
    require(&facts, "rg_version")?;
    for key in [
        "pi_executable",
        "pi_runtime",
        "oo_bin",
        "query_pg_bin",
        "bash_bin",
        "rg_bin",
        "static_library",
        "cjk_font",
    ] {
        require(&facts, key)?;
        require(&facts, &format!("{key}_sha256"))?;
    }
    let cjk = json::parse(cjk_text)?;
    let probe = json::parse(probe_text)?;
    if cjk.get("ranges").is_none() || probe.get("ok").is_none() {
        return Err("cjk or probe json is missing a required field".to_owned());
    }
    let executable = fact(&facts, "pi_executable")?;
    let runtime = fact(&facts, "pi_runtime")?;
    let oo = fact(&facts, "oo_bin")?;
    let query_pg = fact(&facts, "query_pg_bin")?;
    let bash = fact(&facts, "bash_bin")?;
    let rg = fact(&facts, "rg_bin")?;
    let document = json::obj(vec![
        ("contract", json::string("oo.pi-runtime-kit/v1")),
        ("owner_uuid", json::string("eef831aa-dba4-4af2-a78b-541bc9e0a406")),
        ("consumer_uuid", json::string("eef831aa-dba4-4af2-a78b-541bc9e0a397")),
        ("consumer_label", json::string("a397")),
        ("parent_accepted", json::boolean(false)),
        ("goal_accepted", json::boolean(false)),
        ("closes_sp10_w04", json::boolean(false)),
        ("closes_sp10_w05", json::boolean(false)),
        ("qa_complete", json::boolean(false)),
        ("production_signed", json::boolean(false)),
        ("runsc", json::boolean(false)),
        ("deployment", json::string("localprocess")),
        ("mcp", json::boolean(false)),
        ("runtime_download", json::boolean(false)),
        ("profile", json::string(fact(&facts, "profile")?)),
        ("image", json::string(fact(&facts, "image")?)),
        ("export_volume", json::string(volume)),
        ("export_mount", json::string("/opt/pi-runtime-kit")),
        ("export_mountpoint", json::string(mountpoint)),
        ("source_commit", json::string(fact(&facts, "source_commit")?)),
        ("pi_version", json::string(fact(&facts, "pi_version")?)),
        ("pi_commit", json::string(fact(&facts, "pi_commit")?)),
        ("node_version", json::string(fact(&facts, "node_version")?)),
        ("bash_version", json::string(fact(&facts, "bash_version")?)),
        ("rg_version", json::string(fact(&facts, "rg_version")?)),
        ("public_marker", json::string(crate::PUBLIC_MARKER)),
        ("marker_location", json::string("models.json apiKey only")),
        ("paths", paths(&facts)?),
        ("sha256", hashes(&facts)?),
        ("commands", commands(runtime, executable, oo, query_pg, bash, rg)),
        ("static_library", json::obj(vec![
            ("path", json::string(fact(&facts, "static_library")?)),
            ("sha256", json::string(fact(&facts, "static_library_sha256")?)),
            ("linked_into", json::string(query_pg)),
            ("runtime_shared_object", json::string("absent")),
        ])),
        ("cjk", cjk),
        ("probe", probe),
        ("consume", json::obj(vec![
            ("mount_volume", json::string(volume)),
            ("mount_at", json::string("/opt/pi-runtime-kit")),
            ("read_only", json::boolean(true)),
            ("image", json::string(fact(&facts, "image")?)),
            ("pi_executable", json::string(executable)),
            ("pi_executable_sha256", json::string(fact(&facts, "pi_executable_sha256")?)),
            ("pi_runtime", json::string(runtime)),
            ("pi_runtime_sha256", json::string(fact(&facts, "pi_runtime_sha256")?)),
            ("oo_bin", json::string(oo)),
            ("query_pg_bin", json::string(query_pg)),
            ("path_dir", json::string(parent_dir(runtime)?)),
            ("bash", json::string(bash)),
            ("rg", json::string(rg)),
            ("argv_tail", json::array(vec![
                json::string("--mode"),
                json::string("rpc"),
                json::string("--session-dir"),
                json::string("<private-session-dir>"),
                json::string("--provider"),
                json::string("grok"),
                json::string("--model"),
                json::string("grok-4.7"),
                json::string("--no-extensions"),
                json::string("--no-mcp"),
                json::string("--no-skills"),
                json::string("--no-prompt-templates"),
                json::string("--no-context-files"),
            ])),
        ])),
    ]);
    Ok(format!("{}\n", document.compact()))
}

fn paths(facts: &BTreeMap<String, String>) -> Result<Json, String> {
    Ok(json::obj(vec![
        ("pi_executable", json::string(fact(facts, "pi_executable")?)),
        ("pi_runtime", json::string(fact(facts, "pi_runtime")?)),
        ("oo_bin", json::string(fact(facts, "oo_bin")?)),
        ("query_pg_bin", json::string(fact(facts, "query_pg_bin")?)),
        ("bash", json::string(fact(facts, "bash_bin")?)),
        ("rg", json::string(fact(facts, "rg_bin")?)),
        ("static_library", json::string(fact(facts, "static_library")?)),
        ("cjk_font", json::string(fact(facts, "cjk_font")?)),
    ]))
}

fn hashes(facts: &BTreeMap<String, String>) -> Result<Json, String> {
    Ok(json::obj(vec![
        ("pi_executable", json::string(fact(facts, "pi_executable_sha256")?)),
        ("pi_runtime", json::string(fact(facts, "pi_runtime_sha256")?)),
        ("oo_bin", json::string(fact(facts, "oo_bin_sha256")?)),
        ("query_pg_bin", json::string(fact(facts, "query_pg_bin_sha256")?)),
        ("bash", json::string(fact(facts, "bash_bin_sha256")?)),
        ("rg", json::string(fact(facts, "rg_bin_sha256")?)),
        ("static_library", json::string(fact(facts, "static_library_sha256")?)),
        ("cjk_font", json::string(fact(facts, "cjk_font_sha256")?)),
    ]))
}

fn commands(runtime: &str, executable: &str, oo: &str, query_pg: &str, bash: &str, rg: &str) -> Json {
    json::obj(vec![
        ("pi_version", json::array(vec![json::string(runtime), json::string(executable), json::string("--version")])),
        (
            "rpc",
            json::array(vec![
                json::string(runtime),
                json::string(executable),
                json::string("--mode"),
                json::string("rpc"),
                json::string("--session-dir"),
                json::string("<private-session-dir>"),
                json::string("--provider"),
                json::string("grok"),
                json::string("--model"),
                json::string("grok-4.7"),
                json::string("--no-extensions"),
                json::string("--no-mcp"),
                json::string("--no-skills"),
                json::string("--no-prompt-templates"),
                json::string("--no-context-files"),
            ]),
        ),
        ("oo_help", json::array(vec![json::string(oo), json::string("--help")])),
        ("query_pg_help", json::array(vec![json::string(query_pg), json::string("--help")])),
        ("bash_version", json::array(vec![json::string(bash), json::string("--version")])),
        ("rg_version", json::array(vec![json::string(rg), json::string("--version")])),
    ])
}

fn fact<'a>(facts: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    facts.get(key).map(String::as_str).ok_or_else(|| format!("facts missing {key}"))
}

fn require(facts: &BTreeMap<String, String>, key: &str) -> Result<(), String> {
    fact(facts, key).map(|_| ())
}

fn parent_dir(path: &str) -> Result<String, String> {
    let parent = std::path::Path::new(path).parent().ok_or("runtime has no directory")?;
    parent.to_str().map(str::to_owned).ok_or_else(|| "runtime parent is not utf-8".to_owned())
}
