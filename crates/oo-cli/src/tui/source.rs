//! Screen 1 data, pure and testable: the engines and versions the product
//! supports (from the generated support matrix), "paste anything" parsing
//! (URLs, JDBC, `.env`, `host:port`, TNS descriptors), imports from existing
//! client configuration (`~/.pgpass`, `~/.my.cnf`, `tnsnames.ora`, `PG*` /
//! `MYSQL_*` / `ORACLE_*` variables), recent connections without secrets, and
//! a path completer. Nothing here opens a network connection.
//!
//! Passwords found anywhere are held only to be moved into the secret store
//! (or referenced as `env:` / `file:`); `Debug` never prints them and no
//! render path receives them.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

// ---------------------------------------------------------------- engines

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Engine {
    Postgres,
    Mysql,
    Oracle,
}

impl Engine {
    pub const ALL: [Engine; 3] = [Engine::Postgres, Engine::Mysql, Engine::Oracle];

    /// Stack `engine` value.
    pub fn id(self) -> &'static str {
        match self {
            Engine::Postgres => "postgres",
            Engine::Mysql => "mysql",
            Engine::Oracle => "oracle",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Engine::Postgres => "PostgreSQL",
            Engine::Mysql => "MySQL",
            Engine::Oracle => "Oracle",
        }
    }

    /// Two-letter badge drawn as `▐PG▌`.
    pub fn badge(self) -> &'static str {
        match self {
            Engine::Postgres => "PG",
            Engine::Mysql => "My",
            Engine::Oracle => "Or",
        }
    }

    pub fn port(self) -> u16 {
        match self {
            Engine::Postgres => 5432,
            Engine::Mysql => 3306,
            Engine::Oracle => 1521,
        }
    }

    /// The database (Oracle: service) offered before anything is known.
    pub fn default_database(self) -> &'static str {
        match self {
            Engine::Postgres => "postgres",
            Engine::Mysql => "",
            Engine::Oracle => "ORCLPDB1",
        }
    }

    /// Database names worth offering for this engine, most likely first.
    pub fn database_hints(self) -> &'static [&'static str] {
        match self {
            Engine::Postgres => &["postgres"],
            Engine::Mysql => &[],
            Engine::Oracle => &["ORCLPDB1", "FREEPDB1", "XEPDB1", "ORCL", "XE"],
        }
    }

    /// The read-only account name suggested when none is known.
    pub fn suggested_user(self) -> &'static str {
        match self {
            Engine::Oracle => "OO_READER",
            _ => "oo_reader",
        }
    }

    /// Environment variable conventionally holding this engine's password.
    pub fn password_env(self) -> &'static str {
        match self {
            Engine::Postgres => "PGPASSWORD",
            Engine::Mysql => "MYSQL_PWD",
            Engine::Oracle => "ORACLE_PASSWORD",
        }
    }

    /// TLS here goes through the stack CA contract (`tls_ca`).
    pub fn uses_ca(self) -> bool {
        !matches!(self, Engine::Oracle)
    }

    pub fn from_id(id: &str) -> Option<Engine> {
        match id.to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" | "pg" | "pgsql" => Some(Engine::Postgres),
            "mysql" | "my" => Some(Engine::Mysql),
            "oracle" | "ora" => Some(Engine::Oracle),
            _ => None,
        }
    }

    pub fn from_port(port: u16) -> Option<Engine> {
        Engine::ALL.into_iter().find(|e| e.port() == port)
    }
}

// --------------------------------------------------------------- versions

/// One tested version from the generated support matrix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub engine: Engine,
    /// Picker label: `17`, `8.4`, `19c`.
    pub label: String,
    /// Exact version the evidence ran against.
    pub tested: String,
    /// Level of the full snapshot capability.
    pub full: String,
    /// Level of log-based change capture.
    pub cdc: String,
}

const MATRIX: &str =
    include_str!("../../../../docs/database-support/support-matrix.generated.json");

fn label_for(engine: Engine, target: &str, version: &str) -> String {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or("");
    match engine {
        Engine::Postgres => major.to_owned(),
        Engine::Mysql => format!("{major}.{}", parts.next().unwrap_or("0")),
        Engine::Oracle => target
            .split('-')
            .nth(1)
            .filter(|s| s.chars().next().is_some_and(|c| c.is_ascii_digit()))
            .map_or_else(|| major.to_owned(), str::to_owned),
    }
}

/// Supported versions per engine, oldest first, from the generated matrix.
/// Engines absent from the matrix are not offered.
pub fn versions() -> Vec<Version> {
    let value: Value = serde_json::from_str(MATRIX).unwrap_or_default();
    let mut out: Vec<Version> = Vec::new();
    for t in value["targets"].as_array().into_iter().flatten() {
        let target = t["target"].as_str().unwrap_or("");
        let Some(engine) = target.split('-').next().and_then(|f| match f {
            "mysql57" | "mysql8" | "mysql9" => Some(Engine::Mysql),
            other => Engine::from_id(other),
        }) else {
            continue;
        };
        let version = t["version"].as_str().unwrap_or("");
        let level = |cap: &str| t["caps"][cap]["level"].as_str().unwrap_or("").to_owned();
        let v = Version {
            engine,
            label: label_for(engine, target, version),
            tested: version.to_owned(),
            full: level("full"),
            cdc: level("cdc"),
        };
        if !out.iter().any(|o| o.engine == engine && o.label == v.label) {
            out.push(v);
        }
    }
    out
}

pub fn versions_of(engine: Engine) -> Vec<Version> {
    versions()
        .into_iter()
        .filter(|v| v.engine == engine)
        .collect()
}

/// `13–18`, `5.7–9.5`, `11g–23ai`.
pub fn version_range(engine: Engine) -> String {
    let v = versions_of(engine);
    match (v.first(), v.last()) {
        (Some(a), Some(b)) if a.label != b.label => format!("{}–{}", a.label, b.label),
        (Some(a), _) => a.label.clone(),
        _ => String::new(),
    }
}

/// The engine's overall level: verified if every version's full snapshot is.
pub fn engine_level(engine: Engine) -> &'static str {
    let v = versions_of(engine);
    if !v.is_empty() && v.iter().all(|v| v.full == "verified") {
        "verified"
    } else if v.iter().any(|v| v.full == "verified") {
        "partly verified"
    } else {
        "experimental"
    }
}

/// Index into `versions_of(engine)` for a server-reported version string
/// (`17.11`, `PostgreSQL 17.11 on …`, `8.4.7-log`, `19.19.0.0.0`).
pub fn match_version(engine: Engine, raw: &str) -> Option<usize> {
    let number: String = raw
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|s| s.chars().next().is_some_and(|c| c.is_ascii_digit()))?
        .to_owned();
    let mut parts = number.split('.');
    let major = parts.next()?;
    let minor = parts.next().unwrap_or("0");
    let wanted = match engine {
        Engine::Postgres => major.to_owned(),
        Engine::Mysql => format!("{major}.{minor}"),
        Engine::Oracle => match major {
            "11" => "11g".into(),
            "12" | "18" | "19" => format!("{major}c"),
            "21" => "21c".into(),
            "23" => "23ai".into(),
            other => other.into(),
        },
    };
    versions_of(engine).iter().position(|v| v.label == wanted)
}

// ---------------------------------------------------------------- parsing

/// Where a prefill came from, for "what was understood".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Url,
    Jdbc,
    DotEnv,
    HostPort,
    Tns,
}

impl Kind {
    pub fn text(self, zh: bool) -> &'static str {
        match (self, zh) {
            (Kind::Url, false) => "connection URL",
            (Kind::Url, true) => "连接 URL",
            (Kind::Jdbc, false) => "JDBC URL",
            (Kind::Jdbc, true) => "JDBC URL",
            (Kind::DotEnv, false) => ".env line",
            (Kind::DotEnv, true) => ".env 变量",
            (Kind::HostPort, false) => "host:port",
            (Kind::HostPort, true) => "主机:端口",
            (Kind::Tns, false) => "TNS descriptor",
            (Kind::Tns, true) => "TNS 描述符",
        }
    }
}

/// What a pasted text, an import or a recent entry says about a source.
/// Every field is optional; the form fills the rest with defaults.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Prefill {
    pub engine: Option<Engine>,
    /// A recognised but unsupported engine (`mariadb`, `sqlserver`, …).
    pub unsupported: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub database: String,
    /// Oracle: `database` is a SID, not a service name.
    pub sid: bool,
    pub user: String,
    /// Inline password (moved into the secret store, never shown).
    pub password: Option<String>,
    /// `env:VAR` or `file:/path` reference instead of a value.
    pub password_ref: Option<String>,
    /// CA bundle path (`sslrootcert=`, `PGSSLROOTCERT`, `ssl-ca`).
    pub ca_file: String,
    /// Secret-store name of a CA saved by an earlier run.
    pub ca_ref: String,
    pub kind: Kind,
}

impl std::fmt::Debug for Prefill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Prefill({:?} {}:{:?} / {}{} as {}, password {}, ref {:?}, ca {:?}, {:?})",
            self.engine,
            self.host,
            self.port,
            self.database,
            if self.sid { " (SID)" } else { "" },
            self.user,
            if self.password.is_some() {
                "set"
            } else {
                "unset"
            },
            self.password_ref,
            self.ca_file,
            self.kind,
        )
    }
}

impl Prefill {
    /// Port or the engine default.
    pub fn port_or_default(&self) -> Option<u16> {
        self.port.or(self.engine.map(Engine::port))
    }

    /// `user@host:port/db · ••••••` for list rows; never the password.
    pub fn compact(&self) -> String {
        let user = if self.user.is_empty() {
            String::new()
        } else {
            format!("{}@", self.user)
        };
        let port = self
            .port_or_default()
            .map(|p| format!(":{p}"))
            .unwrap_or_default();
        let db = if self.database.is_empty() {
            String::new()
        } else {
            format!("/{}", self.database)
        };
        let secret = match (&self.password, &self.password_ref) {
            (Some(_), _) => format!(" · {MASK}"),
            (None, Some(r)) if r.starts_with("env:") => format!(" · ${}", &r[4..]),
            (None, Some(r)) if r.starts_with("file:") => " · file".into(),
            (None, Some(_)) => format!(" · {MASK}"),
            _ => String::new(),
        };
        format!("{user}{}{port}{db}{secret}", self.host)
    }

    /// One line saying what was understood, never the password.
    pub fn summary(&self, zh: bool) -> String {
        let mut parts = Vec::new();
        if let Some(e) = self.engine {
            parts.push(e.name().to_owned());
        } else if let Some(u) = &self.unsupported {
            parts.push(if zh {
                format!("{u}（暂不支持）")
            } else {
                format!("{u} (not supported yet)")
            });
        }
        if !self.host.is_empty() {
            let port = self
                .port_or_default()
                .map(|p| format!(":{p}"))
                .unwrap_or_default();
            let db = if self.database.is_empty() {
                String::new()
            } else {
                format!("/{}", self.database)
            };
            let tag = match (self.sid, zh) {
                (true, false) => " (SID)",
                (true, true) => "（SID）",
                _ => "",
            };
            parts.push(format!("{}{port}{db}{tag}", self.host));
        }
        if !self.user.is_empty() {
            parts.push(if zh {
                format!("用户 {}", self.user)
            } else {
                format!("user {}", self.user)
            });
        }
        if self.password.is_some() {
            parts.push(
                if zh {
                    "密码 ••••••"
                } else {
                    "password ••••••"
                }
                .into(),
            );
        } else if let Some(r) = &self.password_ref {
            parts.push(if zh {
                format!("密码取自 {r}")
            } else {
                format!("password from {r}")
            });
        }
        if !self.ca_file.is_empty() {
            parts.push(format!("CA {}", self.ca_file));
        }
        parts.join(" · ")
    }
}

/// Percent-decode (`%40` → `@`). Invalid escapes are kept as typed.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `host`, `host:port`, `[v6]:port`. Port 0 or non-numeric is an error.
fn host_port(s: &str) -> Result<(String, Option<u16>), &'static str> {
    if let Some(rest) = s.strip_prefix('[') {
        let (host, tail) = rest.split_once(']').ok_or("DSN_HOST")?;
        let port = match tail.strip_prefix(':') {
            Some(p) => Some(port(p)?),
            None if tail.is_empty() => None,
            None => return Err("DSN_HOST"),
        };
        return Ok((host.to_owned(), port));
    }
    match s.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => Ok((h.to_owned(), Some(port(p)?))),
        _ => Ok((s.to_owned(), None)),
    }
}

fn port(p: &str) -> Result<u16, &'static str> {
    match p.parse::<u16>() {
        Ok(0) | Err(_) => Err("DSN_PORT"),
        Ok(n) => Ok(n),
    }
}

fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._:".contains(c))
}

/// `scheme://[user[:password]@]host[:port][/database][?k=v&…]`.
pub fn parse_url(raw: &str) -> Result<Prefill, &'static str> {
    let raw = raw.trim();
    let (scheme, rest) = raw.split_once("://").ok_or("DSN_SCHEME")?;
    let scheme = scheme.to_ascii_lowercase();
    let mut p = Prefill::default();
    match Engine::from_id(&scheme) {
        Some(e) => p.engine = Some(e),
        None => match scheme.as_str() {
            "mariadb" | "sqlserver" | "mssql" | "db2" | "kingbase" | "dm" | "dm8" | "mongodb"
            | "redis" => p.unsupported = Some(scheme.clone()),
            _ => return Err("DSN_ENGINE_UNKNOWN"),
        },
    }
    let (rest, query) = match rest.split_once('?') {
        Some((r, q)) => (r, q),
        None => (rest, ""),
    };
    let (auth, location) = match rest.rsplit_once('@') {
        Some((a, l)) => (Some(a), l),
        None => (None, rest),
    };
    if let Some(auth) = auth {
        match auth.split_once(':') {
            Some((u, pw)) => {
                p.user = decode(u);
                p.password = Some(decode(pw)).filter(|s| !s.is_empty());
            }
            None => p.user = decode(auth),
        }
    }
    let (hp, path) = match location.split_once('/') {
        Some((h, d)) => (h, d),
        None => (location, ""),
    };
    // MySQL load-balanced lists `h1,h2` take the first host.
    let hp = hp.split(',').next().unwrap_or("");
    let (host, port) = host_port(hp)?;
    if !host.is_empty() && !valid_host(&host) {
        return Err("DSN_HOST");
    }
    p.host = host;
    p.port = port;
    p.database = decode(path.trim_end_matches('/'));
    for pair in query.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = decode(v);
        match k.to_ascii_lowercase().as_str() {
            "user" | "username" if p.user.is_empty() => p.user = v,
            "password" if p.password.is_none() && !v.is_empty() => p.password = Some(v),
            "sslrootcert" | "sslca" | "ssl-ca" | "ssl_ca" | "trustcertificatekeystoreurl" => {
                p.ca_file = v.trim_start_matches("file:").to_owned()
            }
            "dbname" | "database" if p.database.is_empty() => p.database = v,
            "service_name" | "servicename" => p.database = v,
            "sid" => {
                p.database = v;
                p.sid = true;
            }
            _ => (),
        }
    }
    Ok(p)
}

/// Value of `(KEY = value)` anywhere in a TNS descriptor, case-insensitive.
fn tns_value(desc: &str, key: &str) -> Option<String> {
    let upper = desc.to_ascii_uppercase();
    let needle = format!("({key}");
    let mut from = 0;
    while let Some(at) = upper[from..].find(&needle) {
        let start = from + at + needle.len();
        let tail = &desc[start..];
        let trimmed = tail.trim_start();
        if let Some(value) = trimmed.strip_prefix('=') {
            let end = value.find(')').unwrap_or(value.len());
            let v = value[..end].trim();
            if !v.is_empty() {
                return Some(v.to_owned());
            }
        }
        from = start;
    }
    None
}

/// `(DESCRIPTION=(ADDRESS=(PROTOCOL=TCP)(HOST=h)(PORT=1521))(CONNECT_DATA=(SERVICE_NAME=s)))`.
pub fn parse_tns_descriptor(desc: &str) -> Result<Prefill, &'static str> {
    let host = tns_value(desc, "HOST").ok_or("TNS_HOST")?;
    let mut p = Prefill {
        engine: Some(Engine::Oracle),
        host,
        port: tns_value(desc, "PORT").map(|p| port(&p)).transpose()?,
        kind: Kind::Tns,
        ..Prefill::default()
    };
    if let Some(s) = tns_value(desc, "SERVICE_NAME") {
        p.database = s;
    } else if let Some(s) = tns_value(desc, "SID") {
        p.database = s;
        p.sid = true;
    }
    Ok(p)
}

/// JDBC URLs for PostgreSQL, MySQL/MariaDB and the Oracle thin driver.
pub fn parse_jdbc(raw: &str) -> Result<Prefill, &'static str> {
    let rest = raw
        .trim()
        .strip_prefix("jdbc:")
        .or_else(|| raw.trim().strip_prefix("JDBC:"))
        .ok_or("JDBC_PREFIX")?;
    if let Some(thin) = rest
        .strip_prefix("oracle:thin:")
        .or_else(|| rest.strip_prefix("oracle:oci:"))
    {
        let (creds, target) = match thin.rsplit_once('@') {
            Some((c, t)) => (c, t),
            None => ("", thin),
        };
        let mut p = if target.trim_start().starts_with('(') {
            parse_tns_descriptor(target)?
        } else {
            let target = target.trim_start_matches("//");
            let mut p = Prefill {
                engine: Some(Engine::Oracle),
                ..Prefill::default()
            };
            // host:port/service, host:port:SID, host/service
            let (hp, service) = match target.split_once('/') {
                Some((hp, s)) => (hp, Some(s)),
                None => (target, None),
            };
            match service {
                Some(s) => {
                    let (h, pt) = host_port(hp)?;
                    p.host = h;
                    p.port = pt;
                    p.database = s.split(['?', ':']).next().unwrap_or("").to_owned();
                }
                None => {
                    let parts: Vec<&str> = hp.split(':').collect();
                    match parts.as_slice() {
                        [h, pt, sid] => {
                            p.host = (*h).to_owned();
                            p.port = Some(port(pt)?);
                            p.database = (*sid).to_owned();
                            p.sid = true;
                        }
                        [h, pt] => {
                            p.host = (*h).to_owned();
                            p.port = Some(port(pt)?);
                        }
                        [h] => p.host = (*h).to_owned(),
                        _ => return Err("JDBC_ORACLE"),
                    }
                }
            }
            p
        };
        if let Some((u, pw)) = creds.split_once('/') {
            p.user = u.to_owned();
            p.password = Some(pw.to_owned()).filter(|s| !s.is_empty());
        }
        if !p.host.is_empty() && !valid_host(&p.host) {
            return Err("DSN_HOST");
        }
        p.kind = Kind::Jdbc;
        return Ok(p);
    }
    // jdbc:mysql:loadbalance://, jdbc:mysql:replication:// keep the scheme only.
    let (scheme, tail) = rest.split_once("://").ok_or("JDBC_SCHEME")?;
    let scheme = scheme.split(':').next().unwrap_or(scheme);
    let mut p = parse_url(&format!("{scheme}://{tail}"))?;
    p.kind = Kind::Jdbc;
    Ok(p)
}

/// `KEY=VALUE` lines (`export` and quotes allowed), comments skipped.
pub fn dotenv(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(v);
        out.insert(k.to_owned(), v.to_owned());
    }
    out
}

/// Prefill from `PG*`, `MYSQL_*` or `ORACLE_*` variables. Passwords become
/// `env:` references only when `reference` is set (real environment);
/// pasted `.env` text carries the value instead.
pub fn from_vars(vars: &BTreeMap<String, String>, reference: bool) -> Option<Prefill> {
    let get = |k: &str| vars.get(k).filter(|v| !v.is_empty()).cloned();
    let password = |p: &mut Prefill, keys: &[&str]| {
        for k in keys {
            if let Some(v) = get(k) {
                if reference {
                    p.password_ref = Some(format!("env:{k}"));
                } else {
                    p.password = Some(v);
                }
                return;
            }
        }
    };
    if let Some(host) = get("PGHOST") {
        let mut p = Prefill {
            engine: Some(Engine::Postgres),
            host,
            port: get("PGPORT").and_then(|v| port(&v).ok()),
            database: get("PGDATABASE").unwrap_or_default(),
            user: get("PGUSER").unwrap_or_default(),
            ca_file: get("PGSSLROOTCERT").unwrap_or_default(),
            kind: Kind::DotEnv,
            ..Prefill::default()
        };
        password(&mut p, &["PGPASSWORD"]);
        return Some(p);
    }
    if let Some(host) = get("MYSQL_HOST") {
        let mut p = Prefill {
            engine: Some(Engine::Mysql),
            host,
            port: get("MYSQL_TCP_PORT")
                .or_else(|| get("MYSQL_PORT"))
                .and_then(|v| port(&v).ok()),
            database: get("MYSQL_DATABASE").unwrap_or_default(),
            user: get("MYSQL_USER").unwrap_or_default(),
            kind: Kind::DotEnv,
            ..Prefill::default()
        };
        password(&mut p, &["MYSQL_PWD", "MYSQL_PASSWORD"]);
        return Some(p);
    }
    if let Some(host) = get("ORACLE_HOST") {
        let (database, sid) = match (
            get("ORACLE_SERVICE").or_else(|| get("ORACLE_PDB")),
            get("ORACLE_SID"),
        ) {
            (Some(s), _) => (s, false),
            (None, Some(s)) => (s, true),
            (None, None) => (String::new(), false),
        };
        let mut p = Prefill {
            engine: Some(Engine::Oracle),
            host,
            port: get("ORACLE_PORT").and_then(|v| port(&v).ok()),
            database,
            sid,
            user: get("ORACLE_USER").unwrap_or_default(),
            kind: Kind::DotEnv,
            ..Prefill::default()
        };
        password(&mut p, &["ORACLE_PASSWORD"]);
        return Some(p);
    }
    None
}

/// Keys whose value is a whole connection URL, most specific first.
fn url_keys(vars: &BTreeMap<String, String>) -> Vec<&String> {
    let mut keys: Vec<&String> = vars
        .keys()
        .filter(|k| {
            k.as_str() == "DATABASE_URL"
                || k.ends_with("_DATABASE_URL")
                || k.ends_with("_DB_URL")
                || k.as_str() == "DB_URL"
                || k.ends_with("_DSN")
                || k.as_str() == "DSN"
                || k.ends_with("JDBC_URL")
                || k.ends_with("_URL")
        })
        .collect();
    keys.sort_by_key(|k| match k.as_str() {
        "DATABASE_URL" => 0,
        k if k.ends_with("DATABASE_URL") => 1,
        k if k.ends_with("JDBC_URL") => 2,
        k if k.ends_with("_DSN") || k == "DSN" => 3,
        _ => 4,
    });
    keys
}

/// Parse whatever was pasted: URL, JDBC URL, `.env` text, TNS descriptor,
/// `host:port` or a bare host. The error is a stable code.
pub fn parse_any(text: &str) -> Result<Prefill, &'static str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("PASTE_EMPTY");
    }
    let single = !trimmed.contains('\n');
    if single && trimmed.len() > 5 && trimmed[..5].eq_ignore_ascii_case("jdbc:") {
        return parse_jdbc(trimmed);
    }
    if single && trimmed.starts_with('(') {
        return parse_tns_descriptor(trimmed);
    }
    // `.env` text: one or more KEY=VALUE lines, not a URL with a query.
    let first = trimmed
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("");
    let looks_env = first
        .strip_prefix("export ")
        .unwrap_or(first)
        .split_once('=')
        .is_some_and(|(k, _)| {
            !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    if looks_env && !first.split('=').next().unwrap_or("").contains("://") {
        let vars = dotenv(trimmed);
        for key in url_keys(&vars) {
            if let Ok(mut p) = parse_any(&vars[key]) {
                p.kind = Kind::DotEnv;
                return Ok(p);
            }
        }
        return from_vars(&vars, false).ok_or("PASTE_UNKNOWN");
    }
    if !single {
        return Err("PASTE_UNKNOWN");
    }
    if trimmed.contains("://") {
        return parse_url(trimmed);
    }
    let (host, port) = host_port(trimmed)?;
    if !valid_host(&host) || host.contains(' ') {
        return Err("PASTE_UNKNOWN");
    }
    Ok(Prefill {
        engine: port.and_then(Engine::from_port),
        host,
        port,
        kind: Kind::HostPort,
        ..Prefill::default()
    })
}

pub const MASK: &str = "••••••";

/// The pasted text with every secret replaced by a fixed-width mask, on one
/// line (`⏎` between lines). Neither a password nor its length survives.
pub fn mask(text: &str) -> String {
    let line = |l: &str| -> String {
        let l = l.trim_end_matches('\r');
        // .env: KEY=VALUE whose key names a secret.
        if let Some((k, v)) = l.split_once('=') {
            let key = k.trim().trim_start_matches("export ").to_ascii_uppercase();
            if !k.contains("://")
                && ["PASS", "PWD", "SECRET", "TOKEN"]
                    .iter()
                    .any(|s| key.contains(s))
            {
                return format!("{k}={}", if v.is_empty() { "" } else { MASK });
            }
        }
        let mut out = l.to_owned();
        // scheme://user:password@host — password up to the last '@'.
        if let Some(start) = out.find("://").map(|i| i + 3) {
            let tail = out[start..].to_owned();
            let host_end = tail.find(['/', '?']).unwrap_or(tail.len());
            let auth = &tail[..host_end];
            if let Some(at) = auth.rfind('@')
                && let Some(colon) = auth[..at].find(':')
            {
                out = format!(
                    "{}{}{MASK}{}",
                    &out[..start],
                    &auth[..colon + 1],
                    &tail[at..]
                );
            }
        }
        // jdbc:oracle:thin:user/password@…
        if let Some(i) = out.find("thin:")
            && let Some(at) = out[i..].find('@').map(|a| a + i)
            && let Some(slash) = out[i..at].find('/').map(|s| s + i)
        {
            out = format!("{}{MASK}{}", &out[..=slash], &out[at..]);
        }
        // ?password=… / &password=…
        let lower = out.to_ascii_lowercase();
        if let Some(i) = lower.find("password=") {
            let start = i + "password=".len();
            let end = out[start..]
                .find(['&', ';', ' '])
                .map_or(out.len(), |e| e + start);
            if end > start {
                out = format!("{}{MASK}{}", &out[..start], &out[end..]);
            }
        }
        out
    };
    text.trim()
        .lines()
        .map(line)
        .collect::<Vec<_>>()
        .join(" ⏎ ")
}

// ---------------------------------------------------------------- imports

/// One entry of `~/.pgpass`, unescaped. `*` fields stay `*`.
#[derive(Clone, PartialEq, Eq)]
pub struct PgPass {
    pub host: String,
    pub port: String,
    pub database: String,
    pub user: String,
    pub password: String,
}

impl std::fmt::Debug for PgPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PgPass({}:{}:{}:{}:[redacted])",
            self.host, self.port, self.database, self.user
        )
    }
}

pub fn parse_pgpass(text: &str) -> Vec<PgPass> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let mut fields = Vec::new();
        let mut cur = String::new();
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                }
                ':' if fields.len() < 4 => fields.push(std::mem::take(&mut cur)),
                c => cur.push(c),
            }
        }
        fields.push(cur);
        if let [host, port, database, user, password] = fields.as_slice() {
            out.push(PgPass {
                host: host.clone(),
                port: port.clone(),
                database: database.clone(),
                user: user.clone(),
                password: password.clone(),
            });
        }
    }
    out
}

impl PgPass {
    pub fn prefill(&self) -> Option<Prefill> {
        if self.host == "*" || self.host.is_empty() {
            return None;
        }
        let wild = |s: &str| {
            if s == "*" {
                String::new()
            } else {
                s.to_owned()
            }
        };
        Some(Prefill {
            engine: Some(Engine::Postgres),
            host: self.host.clone(),
            port: port(&self.port).ok(),
            database: wild(&self.database),
            user: wild(&self.user),
            password: Some(self.password.clone()).filter(|p| !p.is_empty()),
            kind: Kind::Url,
            ..Prefill::default()
        })
    }
}

/// `[client]` / `[mysql]` options from a MySQL option file.
pub fn parse_my_cnf(text: &str) -> Option<Prefill> {
    let mut section = String::new();
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with(';')
            || line.starts_with('!')
        {
            continue;
        }
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = s.trim().to_ascii_lowercase();
            continue;
        }
        if !matches!(section.as_str(), "client" | "mysql") {
            continue;
        }
        let (k, v) = line.split_once('=').unwrap_or((line, ""));
        let k = k.trim().replace('_', "-").to_ascii_lowercase();
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(v);
        vars.insert(k, v.to_owned());
    }
    let host = vars.get("host").filter(|h| !h.is_empty())?.clone();
    Some(Prefill {
        engine: Some(Engine::Mysql),
        host,
        port: vars.get("port").and_then(|p| port(p).ok()),
        database: vars.get("database").cloned().unwrap_or_default(),
        user: vars.get("user").cloned().unwrap_or_default(),
        password: vars.get("password").filter(|p| !p.is_empty()).cloned(),
        ca_file: vars.get("ssl-ca").cloned().unwrap_or_default(),
        kind: Kind::Url,
        ..Prefill::default()
    })
}

/// `ALIAS = (DESCRIPTION = …)` entries of a `tnsnames.ora`.
pub fn parse_tnsnames(text: &str) -> Vec<(String, Prefill)> {
    let clean: String = text
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();
    let mut alias = String::new();
    let mut depth = 0usize;
    let mut body = String::new();
    for c in clean.chars() {
        match c {
            '(' => {
                depth += 1;
                body.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                body.push(c);
                if depth == 0 {
                    let name = alias.trim().trim_end_matches('=').trim().to_owned();
                    if !name.is_empty()
                        && let Ok(p) = parse_tns_descriptor(&body)
                    {
                        // Several aliases may share one descriptor: `A, B = (…)`.
                        let first = name.split(',').next().unwrap_or(&name).trim().to_owned();
                        out.push((first, p));
                    }
                    alias.clear();
                    body.clear();
                }
            }
            c if depth > 0 => body.push(c),
            c => alias.push(c),
        }
    }
    out
}

/// A one-key choice offered on the first screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// `~/.pgpass`, `$PGHOST`, `~/.my.cnf`, `tnsnames.ora`, `recent`.
    pub origin: String,
    /// Short name (TNS alias, profile) or empty.
    pub name: String,
    pub prefill: Prefill,
    /// Version label remembered from a recent connection.
    pub version: String,
}

impl Found {
    fn key(&self) -> (Option<Engine>, String, Option<u16>, String, String) {
        let p = &self.prefill;
        (
            p.engine,
            p.host.to_ascii_lowercase(),
            p.port_or_default(),
            p.database.clone(),
            p.user.clone(),
        )
    }
}

/// What the machine already knows. `home` and `vars` are injected so tests
/// never read the real environment.
pub struct Machine<'a> {
    pub home: Option<&'a Path>,
    pub vars: &'a BTreeMap<String, String>,
    pub profile: &'a str,
}

fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 256 * 1024 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Recent connections first (this profile, then others), then client
/// configuration. At most eight, duplicates dropped.
pub fn discover(m: &Machine) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    let mut push = |f: Found| {
        if f.prefill.engine.is_some() && !out.iter().any(|o| o.key() == f.key()) {
            out.push(f);
        }
    };
    if let Some(home) = m.home {
        for f in recent(home, m.profile) {
            push(f);
        }
    }
    if let Some(p) = from_vars(m.vars, true) {
        let origin = match p.engine {
            Some(Engine::Postgres) => "$PGHOST",
            Some(Engine::Mysql) => "$MYSQL_HOST",
            _ => "$ORACLE_HOST",
        };
        push(Found {
            origin: origin.into(),
            name: String::new(),
            prefill: p,
            version: String::new(),
        });
    }
    if let Some(home) = m.home {
        let pgpass = m
            .vars
            .get("PGPASSFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".pgpass"));
        if let Some(text) = read_small(&pgpass) {
            for e in parse_pgpass(&text) {
                if let Some(p) = e.prefill() {
                    push(Found {
                        origin: "~/.pgpass".into(),
                        name: String::new(),
                        prefill: p,
                        version: String::new(),
                    });
                }
            }
        }
        if let Some(p) = read_small(&home.join(".my.cnf")).and_then(|t| parse_my_cnf(&t)) {
            push(Found {
                origin: "~/.my.cnf".into(),
                name: String::new(),
                prefill: p,
                version: String::new(),
            });
        }
    }
    let tns_dirs: Vec<PathBuf> = [
        m.vars.get("TNS_ADMIN").map(PathBuf::from),
        m.vars
            .get("ORACLE_HOME")
            .map(|h| Path::new(h).join("network/admin")),
    ]
    .into_iter()
    .flatten()
    .collect();
    for dir in tns_dirs {
        if let Some(text) = read_small(&dir.join("tnsnames.ora")) {
            for (alias, p) in parse_tnsnames(&text) {
                push(Found {
                    origin: "tnsnames.ora".into(),
                    name: alias,
                    prefill: p,
                    version: String::new(),
                });
            }
        }
    }
    out.truncate(8);
    out
}

// ----------------------------------------------------------------- recent

pub const RECENT_PROTOCOL: &str = "oo.recent-connections/v1";
const RECENT_MAX: usize = 8;

fn recent_path(home: &Path) -> PathBuf {
    home.join(".config/open-ontology/recent-connections.json")
}

/// A connection worth offering again: never a password, only a reference
/// to where it lives (`env:`, `file:` or a secret-store name).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recent {
    pub profile: String,
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub sid: bool,
    pub user: String,
    pub version: String,
    pub password_ref: Option<String>,
    pub ca_file: String,
    /// Secret-store name of the trusted CA.
    pub ca_ref: String,
}

impl Recent {
    fn to_json(&self) -> Value {
        json!({"profile":self.profile,"engine":self.engine.id(),"host":self.host,"port":self.port,
            "database":self.database,"sid":self.sid,"user":self.user,"version":self.version,
            "password_ref":self.password_ref,"ca_file":self.ca_file,"ca_ref":self.ca_ref})
    }

    fn from_json(v: &Value) -> Option<Recent> {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_owned();
        Some(Recent {
            profile: s("profile"),
            engine: Engine::from_id(v["engine"].as_str()?)?,
            host: Some(s("host")).filter(|h| valid_host(h))?,
            port: u16::try_from(v["port"].as_u64()?).ok().filter(|p| *p > 0)?,
            database: s("database"),
            sid: v["sid"].as_bool().unwrap_or(false),
            user: s("user"),
            version: s("version"),
            password_ref: v["password_ref"].as_str().map(str::to_owned),
            ca_file: s("ca_file"),
            ca_ref: Some(s("ca_ref"))
                .filter(|r| r.is_empty() || oo_stack::stack::secret_name(r))?,
        })
    }
}

fn load_recent(home: &Path) -> Vec<Recent> {
    read_small(&recent_path(home))
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v["protocol"] == RECENT_PROTOCOL)
        .and_then(|v| v["entries"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(Recent::from_json)
        .collect()
}

/// Recent connections, this profile first.
pub fn recent(home: &Path, profile: &str) -> Vec<Found> {
    let mut list = load_recent(home);
    list.sort_by_key(|r| r.profile != profile);
    list.into_iter()
        .map(|r| Found {
            origin: "recent".into(),
            name: r.profile.clone(),
            version: r.version.clone(),
            prefill: Prefill {
                engine: Some(r.engine),
                host: r.host,
                port: Some(r.port),
                database: r.database,
                sid: r.sid,
                user: r.user,
                password_ref: r.password_ref,
                ca_file: r.ca_file,
                ca_ref: r.ca_ref,
                kind: Kind::Url,
                ..Prefill::default()
            },
        })
        .collect()
}

/// Remember a connection (newest first, one per profile+endpoint). The file
/// holds no secret values; it is written owner-only.
pub fn remember(home: &Path, entry: &Recent) -> Result<(), &'static str> {
    if entry
        .password_ref
        .as_deref()
        .is_some_and(|r| !oo_stack::stack::secret_reference(r))
    {
        return Err("RECENT_REFERENCE");
    }
    let mut list = load_recent(home);
    list.retain(|r| {
        !(r.profile == entry.profile
            && r.engine == entry.engine
            && r.host == entry.host
            && r.port == entry.port
            && r.database == entry.database)
    });
    list.insert(0, entry.clone());
    list.truncate(RECENT_MAX);
    let path = recent_path(home);
    let dir = path.parent().ok_or("RECENT_IO")?;
    std::fs::create_dir_all(dir).map_err(|_| "RECENT_IO")?;
    let body = json!({"protocol":RECENT_PROTOCOL,
        "entries":list.iter().map(Recent::to_json).collect::<Vec<_>>()});
    let tmp = dir.join(format!(".recent-connections.{}.tmp", std::process::id()));
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|_| "RECENT_IO")?;
        f.write_all(body.to_string().as_bytes())
            .map_err(|_| "RECENT_IO")?;
    }
    std::fs::rename(&tmp, &path).map_err(|_| {
        let _ = std::fs::remove_file(&tmp);
        "RECENT_IO"
    })
}

// ------------------------------------------------------------ completion

/// Tab completion for a file path: the completed text and, when the prefix
/// is ambiguous, the candidates (at most eight, sorted). `~/` expands
/// against `home`; the returned text keeps the `~/` the user typed.
pub fn complete_path(input: &str, home: Option<&Path>) -> (String, Vec<String>) {
    let expanded = match (input.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => h.join(rest).to_string_lossy().into_owned(),
        _ => input.to_owned(),
    };
    let (dir, prefix) = match expanded.rfind('/') {
        Some(i) => (&expanded[..=i], &expanded[i + 1..]),
        None => ("./", expanded.as_str()),
    };
    let Ok(entries) = std::fs::read_dir(if dir.is_empty() { "./" } else { dir }) else {
        return (input.to_owned(), Vec::new());
    };
    let mut names: Vec<(String, bool)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name.starts_with(prefix) && (!name.starts_with('.') || prefix.starts_with('.')))
                .then(|| (name, e.path().is_dir()))
        })
        .collect();
    names.sort();
    if names.is_empty() {
        return (input.to_owned(), Vec::new());
    }
    let common = names
        .iter()
        .skip(1)
        .fold(names[0].0.clone(), |acc, (n, _)| {
            acc.chars()
                .zip(n.chars())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect()
        });
    let mut done = format!("{}{common}", &input[..input.len() - prefix.len()]);
    if names.len() == 1 && names[0].1 {
        done.push('/');
    }
    let options = if names.len() > 1 {
        names
            .iter()
            .take(8)
            .map(|(n, d)| if *d { format!("{n}/") } else { n.clone() })
            .collect()
    } else {
        Vec::new()
    };
    (done, options)
}

/// Loopback or private-network address: the only places where the server's
/// own presented CA may be trusted with one key (local development).
pub fn private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_lists_supported_engines_and_versions() {
        let pg: Vec<String> = versions_of(Engine::Postgres)
            .into_iter()
            .map(|v| v.label)
            .collect();
        assert_eq!(pg, ["13", "14", "15", "16", "17", "18"]);
        let my: Vec<String> = versions_of(Engine::Mysql)
            .into_iter()
            .map(|v| v.label)
            .collect();
        assert_eq!(my, ["5.7", "8.0", "8.4", "9.5"]);
        let ora: Vec<String> = versions_of(Engine::Oracle)
            .into_iter()
            .map(|v| v.label)
            .collect();
        assert_eq!(ora, ["11g", "19c", "21c", "23ai"]);
        assert_eq!(version_range(Engine::Oracle), "11g–23ai");
        assert_eq!(engine_level(Engine::Postgres), "verified");
        let o11 = &versions_of(Engine::Oracle)[0];
        assert_eq!(
            (o11.full.as_str(), o11.cdc.as_str()),
            ("verified", "designed")
        );
    }

    #[test]
    fn detected_versions_map_to_picker_rows() {
        assert_eq!(match_version(Engine::Postgres, "17.11"), Some(4));
        assert_eq!(
            match_version(Engine::Postgres, "PostgreSQL 13.23 on x86_64"),
            Some(0)
        );
        assert_eq!(match_version(Engine::Mysql, "8.4.7-log"), Some(2));
        assert_eq!(match_version(Engine::Mysql, "9.1.0"), None);
        assert_eq!(match_version(Engine::Oracle, "19.19.0.0.0"), Some(1));
        assert_eq!(
            match_version(Engine::Oracle, "Oracle 23.5.0.24.07"),
            Some(3)
        );
        assert_eq!(match_version(Engine::Postgres, "12.1"), None);
    }

    #[test]
    fn urls_parse_with_queries_and_redacted_debug() {
        let p = parse_any(
            "postgresql://oo_reader:s%40cret@db.internal:6543/logistics?sslrootcert=/etc/ca.pem",
        )
        .unwrap();
        assert_eq!(p.engine, Some(Engine::Postgres));
        assert_eq!((p.host.as_str(), p.port), ("db.internal", Some(6543)));
        assert_eq!(
            (p.database.as_str(), p.user.as_str()),
            ("logistics", "oo_reader")
        );
        assert_eq!(p.password.as_deref(), Some("s@cret"));
        assert_eq!(p.ca_file, "/etc/ca.pem");
        assert!(!format!("{p:?}").contains("s@cret"));
        assert!(!p.summary(false).contains("s@cret"));
        assert_eq!(parse_any("mysql://r@[::1]:3307/app").unwrap().host, "::1");
        let m = parse_any("mariadb://r@h/db").unwrap();
        assert_eq!(
            (m.engine, m.unsupported.as_deref()),
            (None, Some("mariadb"))
        );
        assert_eq!(parse_any("ftp://h/db").unwrap_err(), "DSN_ENGINE_UNKNOWN");
        assert_eq!(parse_any("postgres://r@h:0/db").unwrap_err(), "DSN_PORT");
    }

    #[test]
    fn jdbc_variants() {
        let pg = parse_any("jdbc:postgresql://db:5433/sales?user=ro&password=pw").unwrap();
        assert_eq!(
            (
                pg.engine,
                pg.host.as_str(),
                pg.port,
                pg.database.as_str(),
                pg.user.as_str()
            ),
            (Some(Engine::Postgres), "db", Some(5433), "sales", "ro")
        );
        assert_eq!(pg.kind, Kind::Jdbc);
        assert_eq!(pg.password.as_deref(), Some("pw"));
        let my = parse_any("jdbc:mysql://h1:3306,h2:3306/shop?useSSL=true").unwrap();
        assert_eq!((my.engine, my.host.as_str()), (Some(Engine::Mysql), "h1"));
        let lb = parse_any("jdbc:mysql:loadbalance://a,b/shop").unwrap();
        assert_eq!((lb.host.as_str(), lb.database.as_str()), ("a", "shop"));
        let svc = parse_any("jdbc:oracle:thin:@ora.internal:1521/ORCLPDB1").unwrap();
        assert_eq!(
            (
                svc.engine,
                svc.host.as_str(),
                svc.port,
                svc.database.as_str(),
                svc.sid
            ),
            (
                Some(Engine::Oracle),
                "ora.internal",
                Some(1521),
                "ORCLPDB1",
                false
            )
        );
        let slash = parse_any("jdbc:oracle:thin:@//ora:1522/FREEPDB1").unwrap();
        assert_eq!(
            (slash.port, slash.database.as_str()),
            (Some(1522), "FREEPDB1")
        );
        let sid = parse_any("jdbc:oracle:thin:scott/tiger@ora:1521:ORCL").unwrap();
        assert_eq!(
            (sid.database.as_str(), sid.sid, sid.user.as_str()),
            ("ORCL", true, "scott")
        );
        assert_eq!(sid.password.as_deref(), Some("tiger"));
        let desc = parse_any(
            "jdbc:oracle:thin:@(DESCRIPTION=(ADDRESS=(PROTOCOL=TCP)(HOST=ora9)(PORT=1525))(CONNECT_DATA=(SERVICE_NAME=sales.example)))",
        )
        .unwrap();
        assert_eq!(
            (desc.host.as_str(), desc.port, desc.database.as_str()),
            ("ora9", Some(1525), "sales.example")
        );
    }

    #[test]
    fn dotenv_and_host_port() {
        let p = parse_any("# app\nexport DATABASE_URL=\"postgres://app:pw@db:5432/app\"\nOTHER=1")
            .unwrap();
        assert_eq!((p.kind, p.host.as_str()), (Kind::DotEnv, "db"));
        let pg = parse_any("PGHOST=10.0.0.5\nPGUSER=ro\nPGDATABASE=dw\nPGPASSWORD=x").unwrap();
        assert_eq!(
            (pg.engine, pg.user.as_str()),
            (Some(Engine::Postgres), "ro")
        );
        assert_eq!(pg.password.as_deref(), Some("x"));
        let hp = parse_any("10.0.0.5:3306").unwrap();
        assert_eq!((hp.engine, hp.kind), (Some(Engine::Mysql), Kind::HostPort));
        let bare = parse_any("db.internal").unwrap();
        assert_eq!((bare.engine, bare.port), (None, None));
        let v6 = parse_any("[fd00::5]:1521").unwrap();
        assert_eq!(
            (v6.host.as_str(), v6.engine),
            ("fd00::5", Some(Engine::Oracle))
        );
        assert!(parse_any("hello world").is_err());
    }

    #[test]
    fn env_import_references_never_copies() {
        let vars: BTreeMap<String, String> = [
            ("PGHOST", "db"),
            ("PGPORT", "6432"),
            ("PGUSER", "ro"),
            ("PGPASSWORD", "secret"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        let p = from_vars(&vars, true).unwrap();
        assert_eq!(p.password, None);
        assert_eq!(p.password_ref.as_deref(), Some("env:PGPASSWORD"));
        assert_eq!(p.port, Some(6432));
        let my: BTreeMap<String, String> = [("MYSQL_HOST", "m"), ("MYSQL_PWD", "x")]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        assert_eq!(
            from_vars(&my, true).unwrap().password_ref.as_deref(),
            Some("env:MYSQL_PWD")
        );
    }

    #[test]
    fn masks_every_kind_of_secret() {
        for (raw, shown) in [
            ("postgres://u:pw@h/db", "postgres://u:••••••@h/db"),
            (
                "postgres://u:a-much-longer-p@ss@h/db",
                "postgres://u:••••••@h/db",
            ),
            ("postgres://u@h/db", "postgres://u@h/db"),
            (
                "jdbc:oracle:thin:scott/tiger@ora:1521:ORCL",
                "jdbc:oracle:thin:scott/••••••@ora:1521:ORCL",
            ),
            (
                "jdbc:postgresql://h/db?user=u&password=pw&ssl=1",
                "jdbc:postgresql://h/db?user=u&password=••••••&ssl=1",
            ),
            ("PGHOST=h\nPGPASSWORD=pw", "PGHOST=h ⏎ PGPASSWORD=••••••"),
            (
                "DATABASE_URL=mysql://r:pw@h/db",
                "DATABASE_URL=mysql://r:••••••@h/db",
            ),
        ] {
            assert_eq!(mask(raw), shown, "{raw}");
        }
    }

    #[test]
    fn pgpass_mycnf_tnsnames() {
        let entries = parse_pgpass(
            "# comment\ndb.internal:5432:logistics:oo_reader:pa\\:ss\n*:*:*:postgres:x\nbad line\n",
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].password, "pa:ss");
        assert!(!format!("{:?}", entries[0]).contains("pa:ss"));
        let p = entries[0].prefill().unwrap();
        assert_eq!(
            (p.host.as_str(), p.port, p.database.as_str()),
            ("db.internal", Some(5432), "logistics")
        );
        assert!(entries[1].prefill().is_none());

        let cnf = parse_my_cnf(
            "[mysqld]\nport=1\n[client]\nhost = mysql.internal\nport=3307\nuser=reader\npassword=\"p w\"\nssl-ca=/etc/my-ca.pem\n",
        )
        .unwrap();
        assert_eq!(
            (cnf.host.as_str(), cnf.port, cnf.user.as_str()),
            ("mysql.internal", Some(3307), "reader")
        );
        assert_eq!(cnf.password.as_deref(), Some("p w"));
        assert_eq!(cnf.ca_file, "/etc/my-ca.pem");
        assert!(parse_my_cnf("[client]\nuser=x\n").is_none());

        let tns = parse_tnsnames(
            "# prod\nSALES =\n  (DESCRIPTION =\n    (ADDRESS = (PROTOCOL = TCP)(HOST = ora1)(PORT = 1521))\n    (CONNECT_DATA = (SERVICE_NAME = sales)))\n\nLEGACY=(DESCRIPTION=(ADDRESS=(HOST=ora2)(PORT=1526))(CONNECT_DATA=(SID=LEG)))\n",
        );
        assert_eq!(tns.len(), 2);
        assert_eq!(tns[0].0, "SALES");
        assert_eq!(
            (tns[0].1.host.as_str(), tns[0].1.database.as_str()),
            ("ora1", "sales")
        );
        assert_eq!((tns[1].1.port, tns[1].1.sid), (Some(1526), true));
    }

    #[test]
    fn discover_and_remember_round_trip_without_secrets() {
        let root = std::env::temp_dir().join(format!("oo-source-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("tns")).unwrap();
        std::fs::write(root.join(".pgpass"), "pg.internal:5432:dw:ro:pw\n").unwrap();
        std::fs::write(
            root.join("tns/tnsnames.ora"),
            "ORCL=(DESCRIPTION=(ADDRESS=(HOST=ora)(PORT=1521))(CONNECT_DATA=(SERVICE_NAME=ORCLPDB1)))",
        )
        .unwrap();
        let vars: BTreeMap<String, String> = [(
            "TNS_ADMIN".to_owned(),
            root.join("tns").to_string_lossy().into_owned(),
        )]
        .into_iter()
        .collect();
        remember(
            &root,
            &Recent {
                profile: "logistics".into(),
                engine: Engine::Postgres,
                host: "db.internal".into(),
                port: 5432,
                database: "logistics".into(),
                sid: false,
                user: "oo_reader".into(),
                version: "17".into(),
                password_ref: Some("source-password".into()),
                ca_file: String::new(),
                ca_ref: "source-ca".into(),
            },
        )
        .unwrap();
        let found = discover(&Machine {
            home: Some(&root),
            vars: &vars,
            profile: "logistics",
        });
        let origins: Vec<&str> = found.iter().map(|f| f.origin.as_str()).collect();
        assert_eq!(origins, ["recent", "~/.pgpass", "tnsnames.ora"]);
        assert_eq!(found[0].version, "17");
        let file =
            std::fs::read_to_string(root.join(".config/open-ontology/recent-connections.json"))
                .unwrap();
        assert!(!file.contains("pw"));
        assert!(
            remember(
                &root,
                &Recent {
                    password_ref: Some("../escape".into()),
                    ..Recent::from_json(
                        &serde_json::from_str::<Value>(&file).unwrap()["entries"][0]
                    )
                    .unwrap()
                }
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn path_completion() {
        let root = std::env::temp_dir().join(format!("oo-complete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("certs")).unwrap();
        std::fs::write(root.join("certs/ca-root.pem"), "x").unwrap();
        std::fs::write(root.join("certs/ca-int.pem"), "x").unwrap();
        let base = root.to_string_lossy().into_owned();
        assert_eq!(
            complete_path(&format!("{base}/ce"), None).0,
            format!("{base}/certs/")
        );
        let (text, options) = complete_path(&format!("{base}/certs/c"), None);
        assert_eq!(text, format!("{base}/certs/ca-"));
        assert_eq!(options, ["ca-int.pem", "ca-root.pem"]);
        assert_eq!(
            complete_path("~/certs/ca-r", Some(&root)).0,
            "~/certs/ca-root.pem"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn private_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.1.1",
            "172.20.0.2",
            "::1",
            "fd00::1",
        ] {
            assert!(private_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "2001:db8::1", "172.32.0.1"] {
            assert!(!private_ip(ip.parse().unwrap()), "{ip}");
        }
    }
}
