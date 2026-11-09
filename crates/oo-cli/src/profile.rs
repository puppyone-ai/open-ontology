//! Named local delivery profiles. Credentials remain private file references.
use crate::lifecycle;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, &'static str>;

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

pub fn active_path(root: &Path) -> Result<PathBuf> {
    let pointer = root.join("active-profile.json");
    if !pointer.try_exists().map_err(|_| "PROFILE_READ")? {
        return Ok(root.join("config.json"));
    }
    let value = lifecycle::read_json(&pointer, true).map_err(|_| "PROFILE_READ")?;
    let name = value["profile"]
        .as_str()
        .filter(|s| valid_name(s))
        .ok_or("PROFILE_NAME")?;
    lifecycle::private(&root.join("profiles"), true).map_err(|_| "PROFILE_READ")?;
    Ok(root.join("profiles").join(format!("{name}.json")))
}

/// Save an already issued credential reference, then select it. Never replace a name.
pub fn save(root: &Path, name: &str, profile: &Value) -> Result<()> {
    if !valid_name(name) {
        return Err("PROFILE_NAME");
    }
    if profile["schema_version"] != 1
        || profile["token_file"]
            .as_str()
            .is_none_or(|p| !Path::new(p).is_absolute())
        || profile["gateway"]
            .as_str()
            .is_none_or(|g| lifecycle::endpoint(g).is_err())
        || profile.as_object().is_none_or(|o| {
            o.keys().any(|k| {
                ![
                    "schema_version",
                    "gateway",
                    "token_file",
                    "ca_file",
                    "client_pem",
                    "connect_ip",
                    "interface",
                    "identity",
                    "business_timezone",
                ]
                .contains(&k.as_str())
            })
        })
    {
        return Err("PROFILE_INVALID");
    }
    lifecycle::mkdir(root).map_err(|_| "PROFILE_DIRECTORY")?;
    let profiles = root.join("profiles");
    lifecycle::mkdir(&profiles).map_err(|_| "PROFILE_DIRECTORY")?;
    lifecycle::write_new(
        &profiles.join(format!("{name}.json")),
        profile.to_string().as_bytes(),
    )
    .map_err(|_| "PROFILE_EXISTS_OR_WRITE_FAILED")?;
    select(root, name)
}

pub fn select(root: &Path, name: &str) -> Result<()> {
    load(root, name)?;
    lifecycle::atomic(&root.join("active-profile.json"), &json!({"profile": name}))
        .map_err(|_| "PROFILE_WRITE")
}

pub fn load(root: &Path, name: &str) -> Result<Value> {
    if !valid_name(name) {
        return Err("PROFILE_NAME");
    }
    lifecycle::private(root, true).map_err(|_| "PROFILE_DIRECTORY")?;
    lifecycle::private(&root.join("profiles"), true).map_err(|_| "PROFILE_DIRECTORY")?;
    let value = lifecycle::read_json(&root.join("profiles").join(format!("{name}.json")), true)
        .map_err(|_| "PROFILE_READ")?;
    if value["schema_version"] != 1 {
        return Err("PROFILE_INVALID");
    }
    Ok(value)
}

pub fn list(root: &Path) -> Result<Vec<String>> {
    let dir = root.join("profiles");
    if !dir.try_exists().map_err(|_| "PROFILE_READ")? {
        return Ok(vec![]);
    }
    lifecycle::private(root, true).map_err(|_| "PROFILE_DIRECTORY")?;
    lifecycle::private(&dir, true).map_err(|_| "PROFILE_DIRECTORY")?;
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|_| "PROFILE_READ")? {
        let entry = entry.map_err(|_| "PROFILE_READ")?;
        let filename = entry.file_name();
        if let Some(name) = filename
            .to_str()
            .and_then(|s| s.strip_suffix(".json"))
            .filter(|s| valid_name(s))
        {
            lifecycle::private(&entry.path(), false).map_err(|_| "PROFILE_READ")?;
            names.push(name.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

pub(crate) fn dispatch(
    args: &[String],
) -> Option<std::result::Result<crate::args::Invocation, crate::outcome::Failure>> {
    let command = args.first()?.as_str();
    if ![
        "profile",
        "use",
        "mcp-config",
        "check",
        "mcp",
        "mcp-request",
    ]
    .contains(&command)
    {
        return None;
    }
    if args.iter().any(|s| s == "--help" || s == "-h") {
        return Some(Ok(crate::args::Invocation::Help(
            "oo profile list [--json]\noo use NAME [--json]\noo check [--json]\noo mcp-config --agent claude|cursor|codex --profile NAME\noo mcp --stdio --profile NAME\nProfiles store private credential references, never credentials in MCP configuration.\n".into()
        )));
    }
    Some(
        (|| {
            let json = args.last().is_some_and(|s| s == "--json");
            let a = if json { &args[..args.len() - 1] } else { args };
            let root = lifecycle::directory().map_err(|_| "PROFILE_DIRECTORY")?;
            let value = match a {
                [cmd, sub] if cmd == "profile" && sub == "list" => {
                    json!({"profiles": list(&root)?})
                }
                [cmd, name] if cmd == "use" => {
                    select(&root, name)?;
                    json!({"profile": name})
                }
                [cmd, flag, agent, pf, name]
                    if cmd == "mcp-config" && flag == "--agent" && pf == "--profile" =>
                {
                    return Ok(crate::args::Invocation::AdminReport(
                        crate::mcp_config::render(agent, name)?,
                        0,
                    ));
                }
                [cmd] if cmd == "check" => {
                    let p = lifecycle::saved(&root).map_err(|_| "PROFILE_READ")?;
                    let rt = lifecycle::runtime().map_err(|_| "READINESS_RUNTIME")?;
                    let result = rt.block_on(crate::check::readiness(&p))?;
                    let exit = if result["ready"] == true { 0 } else { 1 };
                    return Ok(crate::args::Invocation::AdminReport(
                        result.to_string(),
                        exit,
                    ));
                }
                [cmd, flag, name] if cmd == "mcp-request" && flag == "--profile" => {
                    let p = load(&root, name)?;
                    let rt = lifecycle::runtime().map_err(|_| "MCP_RUNTIME")?;
                    let result = rt.block_on(crate::mcp_config::request(&p))?;
                    return Ok(crate::args::Invocation::AdminReport(result.to_string(), 0));
                }
                [cmd, stdio, flag, name]
                    if cmd == "mcp" && stdio == "--stdio" && flag == "--profile" =>
                {
                    load(&root, name)?;
                    let exe = std::env::current_exe().map_err(|_| "MCP_INSTALL")?;
                    use std::os::unix::process::CommandExt;
                    let _error = std::process::Command::new(exe.with_file_name("mcp-gateway"))
                        .env("OO_MCP_PROFILE", name)
                        .env("OO_MCP_CLI", exe)
                        .env_remove("OO_MCP_GATEWAY_URL")
                        .env_remove("OO_MCP_TOKEN_FILE")
                        .exec();
                    return Err("MCP_INSTALL");
                }
                _ => return Err("DELIVERY_ARGUMENTS"),
            };
            Ok(crate::args::Invocation::AdminReport(value.to_string(), 0))
        })()
        .map_err(crate::outcome::Failure::local),
    )
}
