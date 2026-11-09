//! Native credential lifecycle. Errors never include remote bodies or credentials.
use crate::{args::Invocation, load, outcome::Failure};
use reqwest::{Client, Url};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{IsTerminal, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DAY: u64 = 86_400_000;
pub(crate) const HELP: &str = "oo token issue|enroll --grant-file FILE --output-file FILE [--role ROLE]\noo token list --project PROJECT\noo token renew|rotate|revoke --project PROJECT --hash HASH_PREFIX [--deadline-ms MS] [--output-file FILE]\noo token approve CODE --grant-file FILE [--role ROLE]\nToken commands accept --json; issue, enroll and rotate require a new --output-file (0600).\noo login --gateway URL [--code-stdin | --code - | --device] (code on stdin, else prompted; never argv) [--profile NAME] [--business-timezone ZONE] [--json]\noo whoami [--json]\noo share-prompt --grant-file FILE [--role ROLE] [--agent claude|cursor|codex] [--raw-token]\nConnection: --gateway URL --ca-file FILE --connect-ip IP --interface NAME\nAdmin issuance requires --client-pem FILE. Credentials are saved as private file references.\n";
type Result<T> = std::result::Result<T, Failure>;
fn bad() -> Failure {
    Failure::usage("invalid lifecycle arguments or response")
}
fn local() -> Failure {
    Failure::local("private lifecycle file rejected or unavailable")
}
fn protocol() -> Failure {
    let mut f = Failure::local("invalid lifecycle response");
    f.exit = 7;
    f
}
fn transport() -> Failure {
    let mut f = Failure::local("Gateway request failed; check TLS trust and connectivity");
    f.exit = 6;
    f
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().ok_or_else(protocol)
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}
fn code_ok(s: &str) -> bool {
    s.len() == 12
        && s.starts_with("OO-")
        && s.as_bytes()[7] == b'-'
        && s.as_bytes()[3..7]
            .iter()
            .copied()
            .chain(s.as_bytes()[8..].iter().copied())
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
}
fn token_ok(s: &str) -> bool {
    s.len() == 48
        && s.starts_with("ooct_")
        && s[5..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
/// The one-time code never travels in argv: read it from stdin, or prompt for
/// it on a terminal (stderr carries the prompt; stdout stays machine output).
fn stdin_code() -> Result<String> {
    if std::io::stdin().is_terminal() {
        eprint!("Enrollment code (OO-XXXX-XXXX): ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).map_err(|_| bad())?;
        if line.len() > 15 {
            return Err(bad());
        }
        return read_code(line.as_bytes());
    }
    read_code(std::io::stdin())
}
fn read_code(input: impl Read) -> Result<String> {
    let mut reader = input.take(15);
    let mut input = String::new();
    reader.read_to_string(&mut input).map_err(|_| bad())?;
    let code = input
        .strip_suffix("\r\n")
        .or_else(|| input.strip_suffix('\n'))
        .unwrap_or(&input);
    if !code_ok(code) {
        return Err(bad());
    }
    Ok(code.to_owned())
}
pub(crate) fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| local())
}
pub(crate) fn directory() -> Result<PathBuf> {
    let path = std::env::var_os("OO_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config/open-ontology"))
        })
        .ok_or_else(local)?;
    if !path.is_absolute() || path.parent().is_none() {
        return Err(local());
    }
    Ok(path)
}
pub(crate) fn private(path: &Path, dir: bool) -> Result<()> {
    let m = fs::symlink_metadata(path).map_err(|_| local())?;
    // Reuse the existing safe owner probe rather than introducing unsafe getuid.
    if m.file_type().is_symlink()
        || (if dir { !m.is_dir() } else { !m.is_file() })
        || m.mode() & 0o077 != 0
        || m.uid() != load::process_owner().map_err(|_| local())?
    {
        return Err(local());
    }
    Ok(())
}
pub(crate) fn mkdir(path: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|_| local())?;
    private(path, true)
}
pub(crate) fn read_json(path: &Path, secret: bool) -> Result<Value> {
    if secret {
        private(path, false)?;
    }
    let mut f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| local())?;
    if !f.metadata().map_err(|_| local())?.is_file() {
        return Err(local());
    }
    let mut bytes = Vec::new();
    (&mut f)
        .take(16385)
        .read_to_end(&mut bytes)
        .map_err(|_| local())?;
    if bytes.len() > 16384 {
        return Err(local());
    }
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| local())?;
    if !v.is_object() {
        return Err(local());
    }
    Ok(v)
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| local())?;
    if f.write_all(bytes).and_then(|_| f.sync_all()).is_err() {
        drop(f);
        let _ = fs::remove_file(path);
        return Err(local());
    }
    Ok(())
}
pub(crate) fn atomic(path: &Path, value: &Value) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    write_new(&tmp, value.to_string().as_bytes())?;
    let result = fs::rename(&tmp, path).map_err(|_| local());
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub(crate) fn saved(dir: &Path) -> Result<Value> {
    if !dir.exists() {
        return Ok(json!({"schema_version":1}));
    }
    private(dir, true)?;
    let p = crate::profile::active_path(dir).map_err(|_| local())?;
    if !p.exists() {
        return Ok(json!({"schema_version":1}));
    }
    let v = read_json(&p, true)?;
    if v["schema_version"] != 1 {
        return Err(local());
    }
    if v.as_object().ok_or_else(local)?.keys().any(|key| {
        ![
            "schema_version",
            "gateway",
            "token_file",
            "ca_file",
            "client_pem",
            "connect_ip",
            "interface",
            "business_timezone",
            "identity",
        ]
        .contains(&key.as_str())
    }) {
        return Err(local());
    }
    Ok(v)
}
const CONNECTION: &[(&str, &str)] = &[
    ("--gateway", "gateway"),
    ("--ca-file", "ca_file"),
    ("--client-pem", "client_pem"),
    ("--connect-ip", "connect_ip"),
    ("--interface", "interface"),
];
fn options(args: &[String], command: &str) -> Result<BTreeMap<String, String>> {
    let allowed: &[&str] = match command {
        "login" => &[
            "--code",
            "--code-stdin",
            "--device",
            "--profile",
            "--business-timezone",
            "--json",
        ],
        "whoami" => &["--json"],
        "issue" | "enroll" => &["--grant-file", "--role", "--output-file", "--json"],
        "approve" => &["--grant-file", "--role", "--json"],
        "list" => &["--project", "--json"],
        "renew" => &["--project", "--hash", "--deadline-ms", "--json"],
        "rotate" => &["--project", "--hash", "--output-file", "--json"],
        "revoke" => &["--project", "--hash", "--json"],
        _ => &[
            "--grant-file",
            "--role",
            "--agent",
            "--raw-token",
            "--npm-registry",
            "--json",
        ],
    };
    let mut out = BTreeMap::new();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        if !CONNECTION.iter().any(|(f, _)| f == flag) && !allowed.contains(&flag.as_str()) {
            return Err(bad());
        }
        if flag == "--code" {
            // Only `--code -` (stdin) is accepted; a code in argv is visible to
            // other local users and shell history.
            if it.next().map(String::as_str) != Some("-") {
                return Err(Failure::usage(
                    "LOGIN_CODE_ARGV: pass the code on stdin (--code-stdin or --code -) or at the prompt",
                ));
            }
            out.insert("--code-stdin".to_owned(), "true".to_owned());
            continue;
        }
        let value =
            if ["--code-stdin", "--device", "--json", "--raw-token"].contains(&flag.as_str()) {
                "true".into()
            } else {
                it.next()
                    .filter(|v| {
                        !v.is_empty() && !v.starts_with('-') && !v.chars().any(char::is_control)
                    })
                    .ok_or_else(bad)?
                    .clone()
            };
        if out.insert(flag.clone(), value).is_some() {
            return Err(bad());
        }
    }
    Ok(out)
}
pub(crate) fn endpoint(gateway: &str) -> Result<Url> {
    let u = Url::parse(gateway).map_err(|_| bad())?;
    if u.scheme() != "https"
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || gateway
            .bytes()
            .any(|b| !b.is_ascii_graphic() || b"%@\\?#".contains(&b))
    {
        return Err(bad());
    }
    Ok(u)
}
fn connection(mut profile: Value, opts: &BTreeMap<String, String>) -> Result<Value> {
    if let Some(gateway) = opts.get("--gateway")
        && profile["gateway"]
            .as_str()
            .is_some_and(|old| old.trim_end_matches('/') != gateway.trim_end_matches('/'))
    {
        for key in [
            "token_file",
            "ca_file",
            "client_pem",
            "connect_ip",
            "interface",
        ] {
            profile.as_object_mut().ok_or_else(local)?.remove(key);
        }
    }
    for (flag, key) in CONNECTION {
        if let Some(value) = opts.get(*flag) {
            profile[*key] = json!(value);
        }
    }
    endpoint(text(&profile, "gateway")?)?;
    for key in ["ca_file", "client_pem"] {
        if let Some(p) = profile[key].as_str() {
            profile[key] = json!(std::path::absolute(p).map_err(|_| local())?);
        }
    }
    Ok(profile)
}
pub(crate) struct Http {
    client: Client,
    base: String,
}
impl Http {
    pub(crate) fn new(p: &Value, admin: bool) -> Result<Self> {
        let u = endpoint(text(p, "gateway")?)?;
        let mut b = Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5));
        if let Some(ca) = p["ca_file"].as_str() {
            let certs = reqwest::Certificate::from_pem_bundle(&load::read_ca_file(ca)?)
                .map_err(|_| local())?;
            if certs.is_empty() {
                return Err(local());
            }
            b = b.tls_certs_only(certs);
        }
        if let Some(ip) = p["connect_ip"].as_str() {
            b = b.resolve(
                u.domain().ok_or_else(bad)?,
                std::net::SocketAddr::new(ip.parse().map_err(|_| bad())?, 0),
            );
        }
        if let Some(interface) = p["interface"].as_str() {
            b = b.interface(interface);
        }
        if admin {
            let pem = p["client_pem"]
                .as_str()
                .ok_or_else(|| Failure::auth("approved administrator --client-pem required"))?;
            b = b.identity(
                reqwest::Identity::from_pem(&load::read_client_pem(pem)?).map_err(|_| local())?,
            );
        }
        Ok(Self {
            client: b.build().map_err(|_| transport())?,
            base: u.as_str().trim_end_matches('/').into(),
        })
    }
    pub(crate) async fn call(
        &self,
        path: &str,
        body: Option<Value>,
        token: Option<&str>,
        form: bool,
    ) -> Result<(u16, Value)> {
        let url = format!("{}{path}", self.base);
        let mut r = if body.is_some() {
            self.client.post(url)
        } else {
            self.client.get(url)
        }
        .header("accept", "application/json")
        .header("x-oo-expected-publication-id", "current");
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        if let Some(body) = body {
            if form {
                let mut data = reqwest::Url::parse("https://form.invalid").map_err(|_| bad())?;
                {
                    let mut pairs = data.query_pairs_mut();
                    for (k, v) in body.as_object().ok_or_else(bad)? {
                        pairs.append_pair(k, v.as_str().ok_or_else(bad)?);
                    }
                }
                r = r
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(data.query().unwrap_or_default().to_owned());
            } else {
                r = r
                    .header("content-type", "application/json")
                    .body(body.to_string());
            }
        }
        let mut response = r.send().await.map_err(|_| transport())?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| transport())? {
            if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                return Err(protocol());
            }
            bytes.extend_from_slice(&chunk);
        }
        let v: Value = serde_json::from_slice(&bytes).map_err(|_| protocol())?;
        if !v.is_object() {
            return Err(protocol());
        }
        Ok((status, v))
    }
}
pub(crate) fn success((status, v): (u16, Value)) -> Result<Value> {
    if status == 401 || status == 403 {
        return Err(Failure::auth(
            "Access expired, revoked or refused; ask an administrator to renew access, then run oo login",
        ));
    }
    if !(200..300).contains(&status) || v.get("error").is_some() {
        return Err(Failure::local(
            "Gateway refused the request; remote body withheld",
        ));
    }
    Ok(v)
}
fn version(s: &str) -> Result<[u64; 3]> {
    let v: Vec<_> = s.split('.').collect();
    if v.len() != 3
        || v.iter().any(|p| {
            p.is_empty()
                || (p.len() > 1 && p.starts_with('0'))
                || !p.bytes().all(|b| b.is_ascii_digit())
        })
    {
        return Err(protocol());
    }
    Ok([
        v[0].parse().map_err(|_| protocol())?,
        v[1].parse().map_err(|_| protocol())?,
        v[2].parse().map_err(|_| protocol())?,
    ])
}
fn check_version(v: &Value) -> Result<()> {
    let minimum = text(v, "minimum_cli_version")?;
    if version(VERSION)? < version(minimum)? {
        let mut f = Failure::local(format!(
            "CLI_UPGRADE_REQUIRED: Run npm install -g @reduxxx888/ontology-cli@{minimum}"
        ));
        f.exit = 9;
        return Err(f);
    }
    if v["protocol_version"] != "0.2" {
        let mut f = protocol();
        f.exit = 10;
        return Err(f);
    }
    Ok(())
}
async fn negotiate(dir: &Path, p: &Value, http: &Http) -> Result<()> {
    // Trust bytes, not just paths, bind the anonymous cache.
    let ca = p["ca_file"].as_str().map(load::read_ca_file).transpose()?;
    let key = json!([
        http.base,
        p["connect_ip"],
        p["interface"],
        ca.map(|v| oo_core::api::payload_sha256(&v))
    ]);
    let path = dir.join(format!(
        "native-meta-{}.json",
        oo_core::api::payload_sha256(key.to_string().as_bytes())
    ));
    let time = now();
    if path.exists() {
        private(dir, true)?;
        let cached = read_json(&path, true)?;
        if cached["key"] == key
            && cached["checked_at_ms"]
                .as_u64()
                .is_some_and(|t| time >= t && time - t < DAY)
        {
            return check_version(&cached["meta"]);
        }
    }
    let value = success(http.call("/v1/meta", None, None, false).await?)?;
    let meta = json!({"minimum_cli_version":value["minimum_cli_version"],"protocol_version":value["protocol_version"]});
    check_version(&meta)?;
    mkdir(dir)?;
    atomic(&path, &json!({"key":key,"checked_at_ms":time,"meta":meta}))
}
fn expiry(v: &Value, time: u64) -> Result<Option<&'static str>> {
    let end = v["expires_at_ms"].as_u64().ok_or_else(protocol)?;
    if end <= time {
        return Err(Failure::auth(
            "Access expired; ask an administrator to renew access, then run oo login",
        ));
    }
    Ok((end - time <= 7 * DAY).then_some(
        "Access expires within 7 days; ask an administrator to renew access before expiry.",
    ))
}
async fn identity(http: &Http, token: &str) -> Result<Value> {
    let v = success(
        http.call("/v1/auth/whoami", None, Some(token), false)
            .await?,
    )?;
    for key in ["principal_id", "role", "project_id"] {
        if !valid_id(text(&v, key)?) || text(&v, key)?.contains(token) {
            return Err(protocol());
        }
    }
    if v["protocol_version"] != "0.2" {
        return Err(protocol());
    }
    if let Some(warning) = expiry(&v, now())? {
        let _ = crate::outcome::write_stderr(format!("{warning}\n").as_bytes());
    }
    Ok(
        json!({"principal_id":v["principal_id"],"role":v["role"],"project_id":v["project_id"],"expires_at_ms":v["expires_at_ms"],"protocol_version":v["protocol_version"]}),
    )
}
pub(crate) fn read_token(p: &Value) -> Result<String> {
    let t = load::read_token_file(
        p["token_file"]
            .as_str()
            .ok_or_else(|| Failure::auth("run oo login first"))?,
    )?;
    if !token_ok(&t) {
        return Err(Failure::auth("invalid client token"));
    }
    Ok(t)
}
fn next_interval(interval: u64, error: &str) -> Option<u64> {
    match error {
        "slow_down" => Some(interval.saturating_add(5)),
        "authorization_pending" => Some(interval),
        _ => None,
    }
}
async fn device(http: &Http) -> Result<String> {
    let d = success(
        http.call(
            "/v1/auth/device/code",
            Some(json!({"client_id":"oo-cli"})),
            None,
            true,
        )
        .await?,
    )?;
    let code = text(&d, "device_code")?;
    let user = text(&d, "user_code")?;
    let duration = d["expires_in"]
        .as_u64()
        .filter(|s| (1..=1800).contains(s))
        .ok_or_else(protocol)?;
    let mut interval = d
        .get("interval")
        .map(|v| {
            v.as_u64()
                .filter(|s| (1..=300).contains(s))
                .ok_or_else(protocol)
        })
        .transpose()?
        .unwrap_or(5);
    if code.len() != 64
        || !code
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || user.len() != 9
        || user.as_bytes()[4] != b'-'
        || !user
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || b.is_ascii_uppercase() || b.is_ascii_digit())
        || d["verification_uri"] != format!("{}/v1/auth/device", http.base)
    {
        return Err(protocol());
    }
    let _ = crate::outcome::write_stderr(format!("Visit {}\nDevice code: {user}\nAsk your administrator to approve this code and confirm the role.\n",text(&d,"verification_uri")?).as_bytes());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(duration);
    while tokio::time::Instant::now() + Duration::from_secs(interval) < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        let reply = match http.call("/v1/auth/device/token",Some(json!({"client_id":"oo-cli","device_code":code,"grant_type":"urn:ietf:params:oauth:grant-type:device_code"})),None,true).await { Ok(r)=>r, Err(f) if f.exit == 6 => { interval=interval.saturating_mul(2); continue; }, Err(f)=>return Err(f) };
        if let Some(next) = reply.1["error"]
            .as_str()
            .and_then(|e| next_interval(interval, e))
        {
            interval = next;
        } else {
            return Ok(text(&success(reply)?, "access_token")?.into());
        }
    }
    Err(Failure::auth(
        "Device authorization expired; run oo login --device again",
    ))
}
fn shell(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
/// An npm registry for the install step (a staged or private registry that
/// carries this exact version): http(s) origin and path only, no credentials.
fn registry(value: Option<&String>) -> Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let url = Url::parse(value).map_err(|_| bad())?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(bad());
    }
    Ok(Some(url.as_str().to_owned()))
}
fn prompt(
    gateway: &str,
    material: &str,
    raw: bool,
    agent: Option<&str>,
    expires: u64,
    npm_registry: Option<&str>,
) -> String {
    let mut s = String::new();
    if raw {
        s.push_str("RISK: This prompt contains a long-lived bearer token. Anyone with the chat history can use its granted access. Do not share or log it.\n");
    }
    s.push_str(&format!("Configure Open Ontology read-only data access:\n1. Install: npm install -g {}@reduxxx888/ontology-cli@{VERSION}\n2. Bind: printf '%s\\n' {} | oo {} --gateway {} --{}-stdin\n",npm_registry.map(|r| format!("--registry {} ", shell(r))).unwrap_or_default(),shell(material),if raw {"configure"} else {"login"},shell(gateway),if raw {"token"} else {"code"}));
    if !raw {
        s.push_str(&format!("   This code is single-use; expires at Unix milliseconds {expires}. Do not put it in argv.\n"));
    }
    s.push_str("3. Verify: oo whoami && oo tables\n4. Read instructions: oo agent-skill\nBefore querying, use oo describe to understand tables, entities and metric definitions.\nReport NULL, truncation and failed queries honestly; never substitute zero or invent results.\n");
    if let Some(agent) = agent {
        s.push_str(&format!("MCP setup for {agent}: tools/mcp-gateway is a separate stdio executable (mcp-gateway).\nBuild the reviewed crate: cargo build --manifest-path tools/mcp-gateway/Cargo.toml --locked\nIts current transport accepts only a loopback HTTP Gateway, not this HTTPS Gateway. Do not disable TLS or expose an unauthenticated relay; ask the administrator for an approved loopback endpoint.\nSet OO_MCP_GATEWAY_URL to that endpoint and OO_MCP_TOKEN_FILE to the token_file reference in private config.json; never put token text in MCP configuration.\n"));
        s.push_str(match agent { "claude"=>"Register: claude mcp add --transport stdio open-ontology -- /absolute/path/to/mcp-gateway\n", "cursor"=>"Add mcpServers.open-ontology to .cursor/mcp.json with the absolute executable command and env references above.\n", _=>"Add [mcp_servers.open-ontology] to the Codex config with the absolute executable command and env references above.\n" });
    }
    s
}
async fn execute(command: &str, args: &[String]) -> Result<String> {
    let opts = options(args, command)?;
    let dir = directory()?;
    let mut p = connection(
        if command == "login" {
            json!({"schema_version":1})
        } else {
            saved(&dir)?
        },
        &opts,
    )?;
    let http = Http::new(&p, false)?;
    if command == "login" {
        if !opts.contains_key("--gateway")
            || opts.contains_key("--code-stdin") && opts.contains_key("--device")
        {
            return Err(bad());
        }
        let name = opts
            .get("--profile")
            .map(String::as_str)
            .unwrap_or("default");
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(bad());
        }
        let path = dir.join("tokens").join(format!("{name}.token"));
        if fs::symlink_metadata(&path).is_ok() {
            return Err(Failure::local(
                "PROFILE_EXISTS: profile already exists; choose a new --profile; existing credentials were not replaced",
            ));
        }
        if let Some(zone) = opts.get("--business-timezone") {
            if zone != "UTC"
                && (!zone.contains('/')
                    || !zone
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b)))
            {
                return Err(bad());
            }
            oo_core::tz::resolve_local(
                zone,
                oo_core::tz::CivilLocal {
                    year: 2000,
                    month: 1,
                    day: 1,
                    hour: 0,
                    minute: 0,
                    second: 0,
                    micros: 0,
                },
            )
            .map_err(|_| bad())?;
            p["business_timezone"] = json!(zone);
        }
        negotiate(&dir, &p, &http).await?;
        mkdir(&dir.join("tokens"))?;
        let token = if opts.contains_key("--device") {
            device(&http).await?
        } else {
            let code = stdin_code()?;
            if !code_ok(&code) {
                return Err(bad());
            }
            text(
                &success(
                    http.call("/v1/auth/enroll", Some(json!({"code":code})), None, false)
                        .await?,
                )?,
                "token",
            )?
            .to_owned()
        };
        if !token_ok(&token) {
            return Err(protocol());
        }
        // Redemption is single-use: persist the credential before any further IO.
        write_new(&path, token.as_bytes())?;
        let verification = async {
            let who = identity(&http, &token).await?;
            success(http.call("/v1/tables", None, Some(&token), false).await?)?;
            Ok::<_, Failure>(who)
        }
        .await;
        let who = verification.map_err(|error| {
            let mut failure = Failure::local(format!(
                "LOGIN_VERIFICATION_FAILED: credential saved in tokens/{name}.token (0600); active profile unchanged. Configure the saved token file to retry verification or ask the administrator to revoke it; do not redeem the code again."
            ));
            failure.exit = error.exit;
            failure
        })?;
        p["token_file"] = json!(path);
        p["identity"] = who.clone();
        crate::profile::save(&dir, name, &p).map_err(|_| Failure::local(format!(
            "LOGIN_PROFILE_SAVE_FAILED: credential saved in tokens/{name}.token (0600); active profile unchanged. Configure the saved token file or ask the administrator to revoke it."
        )))?;
        let mut report = who;
        report["read_check"] = json!("passed");
        report["profile"] = json!(name);
        report["share_prompt"] = json!(crate::share_prompt::render(name).map_err(|_| bad())?);
        return Ok(if opts.contains_key("--json") {
            report.to_string()
        } else {
            format!(
                "Logged in as {} ({}); read check passed.\n{}",
                text(&report, "principal_id")?,
                text(&report, "role")?,
                text(&report, "share_prompt")?
            )
        });
    }
    negotiate(&dir, &p, &http).await?;
    if command == "whoami" {
        let who = identity(&http, &read_token(&p)?).await?;
        return Ok(if opts.contains_key("--json") {
            who.to_string()
        } else {
            format!(
                "Identity: {}\nRole: {}\nExpiry (Unix ms): {}\nProtocol: {}",
                text(&who, "principal_id")?,
                text(&who, "role")?,
                who["expires_at_ms"],
                text(&who, "protocol_version")?
            )
        });
    }
    let agent = opts.get("--agent").map(String::as_str);
    if agent.is_some_and(|a| !["claude", "cursor", "codex"].contains(&a)) {
        return Err(bad());
    }
    // Validated before any code is issued.
    let npm_registry = registry(opts.get("--npm-registry"))?;
    let mut grant = read_json(Path::new(opts.get("--grant-file").ok_or_else(bad)?), false)?;
    if let Some(role) = opts.get("--role") {
        grant["role"] = json!(role);
    }
    if !valid_id(text(&grant, "role")?)
        || endpoint(text(&grant, "audience_url")?)?
            .as_str()
            .trim_end_matches('/')
            != http.base
    {
        return Err(bad());
    }
    let raw = opts.contains_key("--raw-token");
    let issued = success(
        Http::new(&p, true)?
            .call(
                "/internal/v1/tokens",
                Some(json!({"operation":if raw {"issue"} else {"enroll"},"grant":grant})),
                None,
                false,
            )
            .await?,
    )?;
    let material = text(&issued, if raw { "token" } else { "code" })?;
    if !(if raw {
        token_ok(material)
    } else {
        code_ok(material)
    }) {
        return Err(protocol());
    }
    let expires = issued["expires_at_ms"].as_u64().ok_or_else(protocol)?;
    let rendered = prompt(
        &http.base,
        material,
        raw,
        agent,
        expires,
        npm_registry.as_deref(),
    );
    Ok(if opts.contains_key("--json") {
        json!({"share_prompt": rendered}).to_string()
    } else {
        rendered
    })
}
async fn token_command(args: &[String]) -> Result<String> {
    let (command, mut rest) = args.split_first().ok_or_else(bad)?;
    if ![
        "issue", "list", "renew", "rotate", "revoke", "enroll", "approve",
    ]
    .contains(&command.as_str())
    {
        return Err(bad());
    }
    let user_code = if command == "approve" {
        let (code, tail) = rest.split_first().ok_or_else(bad)?;
        if code.len() != 9
            || code.as_bytes()[4] != b'-'
            || !code
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 4 || b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return Err(bad());
        }
        rest = tail;
        Some(code)
    } else {
        None
    };
    let opts = options(rest, command)?;
    let dir = directory()?;
    let p = connection(saved(&dir)?, &opts)?;
    let http = Http::new(&p, true)?;
    negotiate(&dir, &p, &http).await?;
    let mut request =
        json!({"operation": if command == "approve" { "device_approve" } else { command }});
    if ["issue", "enroll", "approve"].contains(&command.as_str()) {
        let mut grant = read_json(Path::new(opts.get("--grant-file").ok_or_else(bad)?), false)?;
        if let Some(role) = opts.get("--role") {
            grant["role"] = json!(role);
        }
        if !valid_id(text(&grant, "role")?)
            || endpoint(text(&grant, "audience_url")?)?
                .as_str()
                .trim_end_matches('/')
                != http.base
        {
            return Err(bad());
        }
        request["grant"] = grant;
        if let Some(code) = user_code {
            request["user_code"] = json!(code);
        }
    } else {
        let project = opts
            .get("--project")
            .filter(|v| valid_id(v))
            .ok_or_else(bad)?;
        request["project"] = json!(project);
        if command != "list" {
            let hash = opts
                .get("--hash")
                .filter(|v| {
                    (12..=64).contains(&v.len())
                        && v.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
                .ok_or_else(bad)?;
            request["hash"] = json!(hash);
        }
        if command == "renew" {
            let deadline = opts
                .get("--deadline-ms")
                .and_then(|v| v.parse::<i64>().ok())
                .filter(|v| *v > 0)
                .ok_or_else(bad)?;
            request["deadline_ms"] = json!(deadline);
        }
    }
    // Reserve the destination before consuming an issuance/rotation operation.
    let output = if ["issue", "rotate", "enroll"].contains(&command.as_str()) {
        let path = PathBuf::from(opts.get("--output-file").ok_or_else(bad)?);
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|_| local())?;
        Some((path, file))
    } else {
        None
    };
    let result = http
        .call("/internal/v1/tokens", Some(request), None, false)
        .await;
    let mut value = match result.and_then(|reply| {
        if reply.0 == 400 && reply.1["error"]["message"] == "ambiguous_hash_prefix" {
            return Err(Failure::usage(
                "ambiguous_hash_prefix: use a longer token hash prefix",
            ));
        }
        success(reply)
    }) {
        Ok(v) => v,
        Err(e) => {
            if let Some((path, _)) = output {
                let _ = fs::remove_file(path);
            }
            return Err(e);
        }
    };
    if let Some((path, mut file)) = output {
        let key = if command == "enroll" { "code" } else { "token" };
        let secret = value[key].as_str().filter(|secret| {
            if key == "code" {
                code_ok(secret)
            } else {
                token_ok(secret)
            }
        });
        let Some(secret) = secret else {
            drop(file);
            let cleanup = if fs::remove_file(path).is_ok() {
                "empty output removed"
            } else {
                "empty output cleanup failed"
            };
            let hash_key = if key == "code" {
                "code_sha256"
            } else {
                "token_sha256"
            };
            // Remote fields are untrusted: only a canonical digest may reach diagnostics.
            let recovery = value[hash_key]
                .as_str()
                .filter(|s| {
                    s.len() == 64
                        && s.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
                .map(|hash| format!("{hash_key}={hash}"))
                .unwrap_or_else(|| "hash unavailable; use the administrator audit record".into());
            let mut failure = Failure::local(format!(
                "Credential issued but secret missing or invalid; {cleanup}; {recovery}. Ask the administrator to revoke the issued credential."
            ));
            failure.exit = 7;
            return Err(failure);
        };
        file.write_all(secret.as_bytes()).and_then(|_| file.sync_all()).map_err(|_| Failure::local(
            "Credential issued but private output write failed; ask the administrator to revoke it using the audit record"))?;
        value.as_object_mut().ok_or_else(protocol)?.remove(key);
        value["credential_saved"] = json!(true);
    }
    Ok(value.to_string())
}
pub fn dispatch(args: &[String]) -> Option<Result<Invocation>> {
    if args.first().is_some_and(|a| a == "help")
        && args.len() == 2
        && ["login", "whoami", "share-prompt", "token"].contains(&args[1].as_str())
    {
        return Some(Ok(Invocation::Help(HELP.into())));
    }
    let command = args.first()?.as_str();
    if !["login", "whoami", "share-prompt", "token"].contains(&command) {
        return None;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Some(Ok(Invocation::Help(HELP.into())));
    }
    Some(
        runtime()
            .and_then(|rt| {
                if command == "token" {
                    rt.block_on(token_command(&args[1..]))
                } else {
                    rt.block_on(execute(command, &args[1..]))
                }
            })
            .map(|s| Invocation::AdminReport(s, 0)),
    )
}
pub fn prepare_data(mut args: Vec<String>) -> Result<Vec<String>> {
    if !args.first().is_some_and(|s| {
        [
            "query",
            "tables",
            "describe",
            "validate",
            "sql",
            "freshness",
            "knowledge",
            "cancel",
            "status",
            "operations",
        ]
        .contains(&s.as_str())
    }) || args.iter().any(|s| s == "--help" || s == "-h")
    {
        return Ok(args);
    }
    let end = args.iter().position(|s| s == "--").unwrap_or(args.len());
    if args[..end].windows(2).any(|w| {
        (w[0] == "--transport" && w[1] == "local-development")
            || (w[0] == "--auth" && w[1] == "sandbox")
    }) {
        return Ok(args);
    }
    let dir = directory()?;
    let p = saved(&dir)?;
    let mut opts = BTreeMap::new();
    for (flag, _) in CONNECTION {
        if let Some(i) = args[..end].iter().position(|s| s == flag) {
            opts.insert(flag.to_string(), args.get(i + 1).ok_or_else(bad)?.clone());
        }
    }
    if p["gateway"].is_null() {
        return Ok(args);
    }
    let selected = connection(p, &opts)?;
    let mut extra = Vec::new();
    for (flag, key) in CONNECTION
        .iter()
        .copied()
        .chain([("--token-file", "token_file")])
    {
        if !args[..end].iter().any(|a| a == flag)
            && !(key == "token_file" && args[..end].iter().any(|a| a == "--token-stdin"))
            && let Some(v) = selected[key].as_str()
        {
            extra.extend([flag.into(), v.into()]);
        }
    }
    for (flag, value) in [("--transport", "production"), ("--auth", "external")] {
        if !args[..end].iter().any(|a| a == flag) {
            extra.extend([flag.into(), value.into()]);
        }
    }

    args.splice(end..end, extra);
    Ok(args)
}
pub fn preflight(
    gateway: &str,
    ca: Option<&str>,
    ip: Option<std::net::IpAddr>,
    interface: Option<&str>,
    transport: Option<crate::args::Transport>,
) -> Result<()> {
    if transport != Some(crate::args::Transport::Production) {
        return Ok(());
    }
    let p = json!({"gateway":gateway,"ca_file":ca,"connect_ip":ip.map(|v|v.to_string()),"interface":interface});
    let http = Http::new(&p, false)?;
    runtime()?
        .block_on(negotiate(&directory()?, &p, &http))
        .map_err(|mut f| {
            if f.exit == 6 {
                f.exit = 4;
            }
            f
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_input_is_bounded_and_accepts_only_one_line() {
        for input in ["OO-0000-0000", "OO-0000-0000\n", "OO-0000-0000\r\n"] {
            assert_eq!(
                read_code(input.as_bytes()).ok().as_deref(),
                Some("OO-0000-0000")
            );
        }
        for input in [
            "",
            "OO-0000-0000\n\n",
            "OO-0000-0000\r\nextra",
            "OO-0000-00000",
            "OO-IIII-0000",
        ] {
            assert!(read_code(input.as_bytes()).is_err());
        }
    }
    #[test]
    fn version_and_expiry_boundaries() {
        assert!(
            check_version(&json!({"minimum_cli_version":"0.2.0","protocol_version":"0.2"})).is_ok()
        );
        assert_eq!(
            check_version(&json!({"minimum_cli_version":"0.10.0","protocol_version":"0.2"}))
                .err()
                .unwrap()
                .exit,
            9
        );
        assert_eq!(
            check_version(&json!({"minimum_cli_version":"0.2.0","protocol_version":"9"}))
                .err()
                .unwrap()
                .exit,
            10
        );
        assert!(version("0.2.0;echo").is_err());
        assert!(
            expiry(&json!({"expires_at_ms":7*DAY}), 0)
                .ok()
                .flatten()
                .is_some()
        );
        assert!(
            expiry(&json!({"expires_at_ms":7*DAY+1}), 0)
                .ok()
                .flatten()
                .is_none()
        );
        assert!(expiry(&json!({"expires_at_ms":0}), 0).is_err());
    }
    #[test]
    fn polling_and_prompt_contract() {
        assert_eq!(next_interval(5, "slow_down"), Some(10));
        assert_eq!(next_interval(10, "slow_down"), Some(15));
        assert_eq!(next_interval(15, "authorization_pending"), Some(15));
        assert_eq!(next_interval(15, "access_denied"), None);
        for agent in ["claude", "cursor", "codex"] {
            let p = prompt(
                "https://gateway.example",
                "OO-0000-0000",
                false,
                Some(agent),
                1000,
                None,
            );
            assert!(!p.contains("ooct_"));
            assert!(p.contains("--code-stdin"));
            assert!(p.contains(agent));
        }
        assert!(
            prompt(
                "https://gateway.example",
                "test-material",
                true,
                None,
                1000,
                None
            )
            .starts_with("RISK:")
        );
        let staged = prompt(
            "https://g.example",
            "OO-0000-0000",
            false,
            None,
            1,
            Some("http://localhost:4873/"),
        );
        assert!(staged.contains(&format!(
            "npm install -g --registry 'http://localhost:4873/' @reduxxx888/ontology-cli@{VERSION}"
        )));
        for value in ["ftp://x", "https://u:p@x/", "https://x/?a=1", "not a url"] {
            assert!(registry(Some(&value.to_owned())).is_err());
        }
        assert_eq!(registry(None).ok().flatten(), None);
        assert!(!code_ok("OO-IIII-0000"));
        assert!(code_ok("OO-0000-0000"));
    }
}
