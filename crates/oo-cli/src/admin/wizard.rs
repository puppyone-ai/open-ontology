//! `oo init`: connect, choose tables, then the remaining configuration
//! questions, preflight and an optional build. A saved configuration is not a
//! deployed/reviewed pack. Screens and their visual language live in
//! `crate::tui`; this module supplies the real probes and side effects.
use std::fs::{self, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::tui::app::{self, Live, Term};
use crate::tui::screens::{Check, Status, connect};
use crate::tui::source::{self as src, Engine, Prefill};
use crate::tui::theme::Theme;
use oo_stack::secret::SecretStore;
use serde_json::{Value, json};

use super::{Failure, Invocation, doctor};

/// Product stage (0 connect … 4 deliver) for a legacy wizard step number.
fn stage(step: u8) -> usize {
    match step {
        1 => 0,
        2 => 1,
        3..=5 => 2,
        6 => 3,
        _ => 4,
    }
}

pub(super) struct Screen {
    term: Term,
}

impl Screen {
    pub(super) fn open() -> Result<Self, Failure> {
        Self::with(Theme::from_env(false))
    }

    pub(super) fn with(theme: Theme) -> Result<Self, Failure> {
        Ok(Self {
            term: Term::open(theme).map_err(Failure::local)?,
        })
    }

    pub(super) fn term(&mut self) -> &mut Term {
        &mut self.term
    }

    pub(super) fn prompt(
        &mut self,
        step: u8,
        label: &str,
        default: &str,
        secret: bool,
    ) -> Result<String, Failure> {
        app::prompt(&mut self.term, stage(step), label, default, secret).map_err(Failure::local)
    }

    /// Build progress: the five-stage timeline when the chain state names
    /// stages, otherwise the plain status text. Returns true when the user
    /// asked to stop watching.
    pub(super) fn progress(&mut self, state: &Value, text: &str) -> Result<bool, Failure> {
        use crate::tui::screens::build;
        let timeline = state["stages"].as_array().is_some_and(|s| {
            s.iter()
                .any(|st| st["name"].as_str().and_then(build::product_stage).is_some())
        });
        let view = build::from_chain(state);
        self.term
            .draw(|area, buf, theme| {
                if timeline {
                    build::render(&view, area, buf, theme);
                } else {
                    crate::tui::screens::prompt(2, text, "", "", false, 0, area, buf, theme);
                }
            })
            .map_err(Failure::local)?;
        Ok(self
            .term
            .key(std::time::Duration::ZERO)
            .map_err(Failure::local)?
            .is_some_and(|key| {
                matches!(
                    key.code,
                    crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q')
                ) || (key.code == crossterm::event::KeyCode::Char('c')
                    && key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL))
            }))
    }
}

fn secret(
    screen: &mut Screen,
    store: &SecretStore,
    step: u8,
    label: &str,
    default: &str,
) -> Result<Value, Failure> {
    let name = screen.prompt(
        step,
        &format!("{label} secret name (new file; never overwritten)"),
        default,
        false,
    )?;
    if !oo_stack::stack::secret_name(&name) {
        return Err(Failure::usage("SECRET_NAME"));
    }
    let value = screen.prompt(
        step,
        &format!("{label} (stored immediately, never displayed)"),
        "",
        true,
    )?;
    store
        .write_new(&name, value.as_bytes())
        .map_err(Failure::local)?;
    Ok(json!({"secret_ref":name}))
}

/// Like `secret`, but an owner who already keeps the value in a private file
/// can import it by path instead of typing it.
fn secret_or_import(
    screen: &mut Screen,
    store: &SecretStore,
    step: u8,
    label: &str,
    default: &str,
) -> Result<Value, Failure> {
    let path = screen.prompt(
        step,
        &format!("{label}: private file to import (blank: type the value next)"),
        "",
        false,
    )?;
    if path.is_empty() {
        return secret(screen, store, step, label, default);
    }
    let name = screen.prompt(
        step,
        &format!("{label} secret name (new file; never overwritten)"),
        default,
        false,
    )?;
    if !oo_stack::stack::secret_name(&name) {
        return Err(Failure::usage("SECRET_NAME"));
    }
    import_file(store, &name, Path::new(&path), false)
}

fn list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn import_file(store: &SecretStore, name: &str, path: &Path, ca: bool) -> Result<Value, Failure> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Failure::local("TUI_IMPORT_FILE"))?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= 65_536)
    {
        return Err(Failure::local("TUI_IMPORT_FILE"));
    }
    let mut bytes = Vec::new();
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::local("TUI_IMPORT_FILE"))?;
    if ca {
        oo_connectors::PgTlsConfig::from_ca_pem(&bytes)
            .map_err(|_| Failure::usage("TUI_CA_PEM_REQUIRED"))?;
    }
    if std::str::from_utf8(&bytes).is_err() || bytes.contains(&0) {
        return Err(Failure::usage("TUI_IMPORT_UTF8"));
    }
    store.write_new(name, &bytes).map_err(Failure::local)?;
    Ok(json!({"secret_ref":name}))
}

fn confirmed_pin(observed: &str, confirmation: &str) -> Result<String, Failure> {
    if !oo_stack::stack::fingerprint(observed) || confirmation != observed {
        return Err(Failure::usage("SSH_HOST_KEY_REJECTED"));
    }
    Ok(observed.to_owned())
}

fn source_ssh(screen: &mut Screen, store: &SecretStore) -> Result<Option<Value>, Failure> {
    let host = screen.prompt(1, "SSH jump host (blank: direct)", "", false)?;
    if host.is_empty() {
        return Ok(None);
    }
    let port = screen
        .prompt(1, "SSH port", "22", false)?
        .parse::<u16>()
        .map_err(|_| Failure::usage("STACK_SOURCE_SSH"))?;
    let user = screen.prompt(1, "SSH user", "", false)?;
    if !oo_stack::stack::ssh_host(&host) || port == 0 || !oo_stack::stack::login_name(&user) {
        return Err(Failure::usage("STACK_SOURCE_SSH"));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Failure::local("SSH_RUNTIME"))?;
    // Discovery does not authenticate. Credentials are collected only after the
    // owner checks the key over a separate channel and explicitly pins it.
    let observed = runtime
        .block_on(oo_connectors::transport::ssh::discover(&host, port))
        .map_err(Failure::local)?;
    let answer = screen.prompt(
        1,
        &format!(
            "Observed SSH key: {observed}\nVerify with the host administrator OUT OF BAND.\n\
         Type the full independently verified SHA256 fingerprint to pin it; blank refuses."
        ),
        "",
        false,
    )?;
    let pin = confirmed_pin(&observed, &answer)?;
    let method = screen.prompt(1, "SSH authentication: key / password", "key", false)?;
    let mut ssh = json!({"host":host,"port":port,"user":user,"host_key_sha256":pin});
    match method.as_str() {
        "key" => {
            let path = screen.prompt(
                1,
                "SSH private key file path (unencrypted; imported, never displayed)",
                "",
                false,
            )?;
            let name =
                screen.prompt(1, "SSH key secret name (new file)", "source-ssh-key", false)?;
            ssh["key"] = import_file(store, &name, Path::new(&path), false)?;
        }
        "password" => {
            ssh["password"] = secret(screen, store, 1, "SSH password", "source-ssh-password")?
        }
        _ => return Err(Failure::usage("STACK_SOURCE_SSH")),
    }
    Ok(Some(ssh))
}

fn save_new(stack: &oo_stack::Stack, path: &Path) -> Result<(), Failure> {
    // create_new refuses existing files and symlinks. An aborted wizard never
    // replaces a configuration or deletes credentials the user may have kept.
    let mut bytes = serde_json::to_vec_pretty(stack).map_err(|_| Failure::local("TUI_CONFIG"))?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| Failure::local("TUI_CONFIG_EXISTS_OR_IO"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Failure::local("TUI_CONFIG_IO"))
}

fn build_saved(path: &str, preflight_exit: u8) -> Result<Invocation, Failure> {
    if preflight_exit != 0 {
        return Err(Failure::local(
            "TUI_PREFLIGHT_BLOCKED: configuration saved; resolve oo doctor failures before oo build",
        ));
    }
    // Use the same entry points as the shell: deploy (idempotent), then the
    // runtime build with retained checkpoints, owner review and share prompt.
    let args = ["--config".into(), path.into()];
    let up = super::execute("up", &args)?;
    if matches!(up, Invocation::AdminReport(_, exit) if exit != 0) {
        return Ok(up);
    }
    super::execute("build", &args)
}

/// Options for `oo init` / `oo quickstart` beyond `--config`.
#[derive(Default)]
pub(super) struct InitOptions {
    pub demo: bool,
    pub catalog: Option<std::path::PathBuf>,
    pub tables: Option<Vec<String>>,
    pub tables_recommended: bool,
    pub config: Option<std::path::PathBuf>,
    pub profile: Option<String>,
    pub no_anim: bool,
    pub json: bool,
    /// Screen 1 fields from flags (`--engine`, `--host`, …).
    pub connect: ConnectFlags,
}

/// Every screen 1 field as a flag. Passwords only by reference.
#[derive(Default)]
pub(super) struct ConnectFlags {
    pub dsn: Option<String>,
    pub engine: Option<Engine>,
    pub version: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub sid: bool,
    pub user: Option<String>,
    pub password_ref: Option<String>,
    pub ca_file: Option<String>,
}

impl ConnectFlags {
    pub fn any(&self) -> bool {
        self.dsn.is_some()
            || self.engine.is_some()
            || self.host.is_some()
            || self.port.is_some()
            || self.database.is_some()
            || self.user.is_some()
            || self.password_ref.is_some()
            || self.ca_file.is_some()
            || self.version.is_some()
    }

    /// The flags as a prefill: `--dsn` first, flags override it.
    pub fn prefill(&self) -> Result<Prefill, Failure> {
        let mut p = match &self.dsn {
            Some(text) => src::parse_any(text).map_err(Failure::usage)?,
            None => Prefill::default(),
        };
        if let Some(e) = self.engine {
            p.engine = Some(e);
        }
        if p.engine.is_none() {
            p.engine = self.port.or(p.port).and_then(Engine::from_port);
        }
        if let Some(h) = &self.host {
            p.host = h.clone();
        }
        if let Some(n) = self.port {
            p.port = Some(n);
        }
        if let Some(d) = &self.database {
            p.database = d.clone();
            p.sid = self.sid;
        }
        if let Some(u) = &self.user {
            p.user = u.clone();
        }
        if let Some(r) = &self.password_ref {
            p.password = None;
            p.password_ref = Some(r.clone());
        }
        if let Some(c) = &self.ca_file {
            p.ca_file = c.clone();
        }
        Ok(p)
    }
}

pub(super) fn parse_init(args: &[String], demo: bool) -> Result<InitOptions, Failure> {
    let bad = || Failure::usage("ADMIN_ARGUMENT: invalid init option");
    let mut opts = InitOptions {
        demo,
        ..InitOptions::default()
    };
    let mut seen = std::collections::HashSet::new();
    let mut index = 0;
    while index < args.len() {
        let (key, inline) = match args[index].split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v.to_owned())),
            _ => (args[index].as_str(), None),
        };
        if !seen.insert(key.to_owned()) {
            return Err(Failure::usage("ADMIN_ARGUMENT: duplicate option"));
        }
        let mut value = || -> Result<String, Failure> {
            if let Some(v) = &inline {
                return Ok(v.clone());
            }
            index += 1;
            args.get(index)
                .filter(|s| !s.is_empty() && !s.starts_with('-'))
                .cloned()
                .ok_or_else(bad)
        };
        match key {
            "--demo" => opts.demo = true,
            "--yes" => (),
            "--json" => opts.json = true,
            "--no-anim" => opts.no_anim = true,
            "--tables-recommended" => opts.tables_recommended = true,
            "--catalog" => opts.catalog = Some(value()?.into()),
            "--config" => opts.config = Some(value()?.into()),
            "--profile" => {
                let name = value()?;
                if !crate::tui::backend::profile_name(&name) {
                    return Err(bad());
                }
                opts.profile = Some(name);
            }
            "--tables" => {
                opts.tables = Some(list(&value()?));
            }
            "--dsn" => opts.connect.dsn = Some(value()?),
            "--engine" => {
                opts.connect.engine = Some(Engine::from_id(&value()?).ok_or_else(|| {
                    Failure::usage("ADMIN_ARGUMENT: --engine postgres|mysql|oracle")
                })?)
            }
            "--version" => opts.connect.version = Some(value()?),
            "--host" => opts.connect.host = Some(value()?),
            "--port" => {
                opts.connect.port = Some(
                    value()?
                        .parse()
                        .ok()
                        .filter(|p: &u16| *p > 0)
                        .ok_or_else(bad)?,
                )
            }
            "--database" | "--service" => opts.connect.database = Some(value()?),
            "--sid" => {
                opts.connect.database = Some(value()?);
                opts.connect.sid = true;
            }
            "--user" => opts.connect.user = Some(value()?),
            "--password-env" => opts.connect.password_ref = Some(format!("env:{}", value()?)),
            "--password-file" => {
                let path = std::path::absolute(value()?).map_err(|_| bad())?;
                opts.connect.password_ref = Some(format!("file:{}", path.display()));
            }
            "--ca-file" => opts.connect.ca_file = Some(value()?),
            _ => return Err(bad()),
        }
        index += 1;
    }
    if opts.tables.is_some() && opts.tables_recommended {
        return Err(Failure::usage(
            "ADMIN_ARGUMENT: --tables and --tables-recommended are exclusive",
        ));
    }
    if opts
        .connect
        .password_ref
        .as_deref()
        .is_some_and(|r| !oo_stack::stack::secret_reference(r))
    {
        return Err(Failure::usage("ADMIN_ARGUMENT: invalid password reference"));
    }
    Ok(opts)
}

fn zh() -> bool {
    crate::tui::theme::lang_zh()
}

/// Interface copy in the system language.
fn tr(en: &'static str, zh_text: &'static str) -> &'static str {
    if zh() { zh_text } else { en }
}

fn catalog_tables(
    opts: &InitOptions,
) -> Result<Option<Vec<crate::tui::scorer::TableMeta>>, Failure> {
    if let Some(path) = &opts.catalog {
        let value = crate::tui::adjudication::read_json(path)
            .map_err(|_| Failure::local("TABLE_CATALOG_IO"))?;
        return crate::tui::scorer::from_catalog(&value)
            .map(Some)
            .map_err(Failure::usage);
    }
    Ok(opts.demo.then(crate::tui::screens::select::demo_tables))
}

/// Entry for every `oo init` / `oo quickstart` form except plain `--config`.
pub(super) fn init(opts: &InitOptions) -> Result<Invocation, Failure> {
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal() && !opts.json;
    if opts.tables.is_some() || opts.tables_recommended || (!tty && opts.demo) {
        return select_tables(opts);
    }
    if !tty && opts.connect.any() {
        return connect_check(opts);
    }
    if !tty {
        return Err(Failure::usage(
            "ADMIN_CONFIG_REQUIRED: use --config for non-interactive init",
        ));
    }
    run(opts)
}

/// Non-TTY screen 2: score the catalog and save an explicit list.
fn select_tables(opts: &InitOptions) -> Result<Invocation, Failure> {
    use crate::tui::scorer::{recommended, score};
    let tables = catalog_tables(opts)?.ok_or_else(|| {
        Failure::usage("TABLE_CATALOG_REQUIRED: pass --catalog CATALOG.json or --demo")
    })?;
    let scored = score(&tables);
    let chosen = match &opts.tables {
        Some(list) => {
            if list.iter().any(|t| !scored.iter().any(|s| &s.table == t)) {
                return Err(Failure::usage(
                    "TABLES_UNKNOWN: every table must be in the catalog",
                ));
            }
            let mut list = list.clone();
            list.sort();
            list.dedup();
            list
        }
        None => recommended(&scored),
    };
    if chosen.is_empty() {
        return Err(Failure::usage("TABLES_EMPTY"));
    }
    let saved = match &opts.config {
        Some(path) => {
            save_scope(path, &chosen)?;
            true
        }
        None => false,
    };
    let zh = zh();
    Ok(Invocation::AdminReport(
        if opts.json {
            json!({"protocol":"oo.init-tables/v1","demo":opts.demo,
                "tables":scored.iter().map(|s| json!({"table":s.table,"verdict":s.verdict,
                    "reason":s.reason,"text":s.reason.text(zh),"referenced_by":s.referenced_by,"rows":s.rows})).collect::<Vec<_>>(),
                "selected":chosen,"saved":saved})
            .to_string()
        } else {
            let mut out = String::new();
            for s in &scored {
                let mark = if chosen.contains(&s.table) {
                    "◉"
                } else {
                    "○"
                };
                out.push_str(&format!(
                    "{mark} {} {:<32} {}\n",
                    s.verdict.glyph(),
                    s.table,
                    s.reason.text(zh)
                ));
            }
            out.push_str(&format!(
                "{} of {} tables selected{}",
                chosen.len(),
                scored.len(),
                if saved {
                    "; saved as an explicit scope.include list"
                } else {
                    ""
                }
            ));
            out
        },
        0,
    ))
}

/// Replace the first source's `scope.include` with an explicit list.
fn save_scope(path: &Path, tables: &[String]) -> Result<(), Failure> {
    let stack = super::read_stack(path)?;
    let mut value = serde_json::to_value(&stack).map_err(|_| Failure::local("ADMIN_CONFIG_IO"))?;
    let source = value["sources"]
        .get_mut(0)
        .ok_or_else(|| Failure::usage("STACK_SOURCE_REQUIRED"))?;
    source["scope"]["include"] = json!(tables);
    let stack = oo_stack::parse(&value.to_string()).map_err(Failure::usage)?;
    let mut bytes =
        serde_json::to_vec_pretty(&stack).map_err(|_| Failure::local("ADMIN_CONFIG_IO"))?;
    bytes.push(b'\n');
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = parent.join(format!(".oo-stack-{}.tmp", std::process::id()));
    let result = save_new_bytes(&temp, &bytes)
        .and_then(|()| fs::rename(&temp, path).map_err(|_| Failure::local("ADMIN_CONFIG_IO")));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn save_new_bytes(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| Failure::local("ADMIN_CONFIG_IO"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Failure::local("ADMIN_CONFIG_IO"))
}

/// Store a secret under the first free name `base`, `base-2`, ...
fn store_new(store: &SecretStore, base: &str, value: &[u8]) -> Result<Value, Failure> {
    for n in 1..=20 {
        let name = if n == 1 {
            base.to_owned()
        } else {
            format!("{base}-{n}")
        };
        if store.write_new(&name, value).is_ok() {
            return Ok(json!({"secret_ref":name}));
        }
    }
    Err(Failure::local("SECRET_NAME_EXHAUSTED"))
}

/// Screen 1 sign-in. PostgreSQL and Oracle sign in through the same
/// connectors the build uses (read-only, bounded) and report the server
/// version; MySQL's version comes from its greeting and its sign-in runs in
/// `oo doctor` through the pinned helper. Nothing is written anywhere.
struct WizardLive<'a> {
    store: &'a SecretStore,
}

impl Live for WizardLive<'_> {
    fn verify(&self, request: &connect::Request) -> connect::Verified {
        verify(self.store, request)
    }
}

fn request_password(store: &SecretStore, r: &connect::Request) -> Result<String, String> {
    match &r.secret {
        connect::Secret::Typed(p) => Ok(p.clone()),
        connect::Secret::Ref(name) => store.read(name).map_err(str::to_owned),
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "TUI_RUNTIME".to_owned())
}

/// Catalog reads from the wizard: short statements, room for the
/// discovery bound (10 000 tables plus its sentinel).
fn wizard_budget() -> oo_connectors::engine::ReadBudget {
    oo_connectors::engine::ReadBudget {
        statement_timeout_ms: 15_000,
        ..Default::default()
    }
}

/// The same read-only PostgreSQL connector the build uses.
fn pg_engine(
    store: &SecretStore,
    r: &connect::Request,
    password: String,
) -> Result<oo_connectors::postgres::PostgresEngine, String> {
    use oo_connectors::{
        PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SourceId,
        TableSelection,
    };
    let ca = match (&r.ca_pem, &r.ca_ref) {
        (Some(pem), _) => pem.clone(),
        (None, Some(name)) => store.read(name).map_err(str::to_owned)?,
        (None, None) => return Err("SOURCE_TLS_CA_REQUIRED".to_owned()),
    };
    let config = PgSourceConfig::try_new(
        SourceId::new("source").map_err(|e| e.to_string())?,
        PgEndpoint::new(&r.host, r.port, &r.database).map_err(|e| e.to_string())?,
        PgCredentials::new(r.user.clone(), password).map_err(|e| e.to_string())?,
        SecretRef::new("secret://source-password").map_err(|e| e.to_string())?,
        PgTlsConfig::from_ca_pem(ca).map_err(|e| e.to_string())?,
        PgBudget::new(
            std::time::Duration::from_secs(8),
            std::time::Duration::from_secs(15),
            std::time::Duration::from_secs(1),
            10_001,
            16 * 1024 * 1024,
        )
        .map_err(|e| e.to_string())?,
        TableSelection::new(Vec::new()).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(oo_connectors::postgres::PostgresEngine::new(config))
}

fn oracle_engine(
    r: &connect::Request,
    password: String,
) -> Result<oo_connectors::oracle::engine::OracleEngine, oo_connectors::SourceError> {
    use oo_connectors::oracle::engine::{OracleConnectConfig, OracleEngine};
    OracleEngine::connect(OracleConnectConfig::new(
        &r.host,
        r.port,
        &r.database,
        &r.user,
        password.into(),
    ))
}

fn verify(store: &SecretStore, r: &connect::Request) -> connect::Verified {
    let mut out = connect::Verified {
        generation: r.generation,
        auth: Err(String::new()),
        checked: false,
        version: None,
        databases: Vec::new(),
    };
    let password = match request_password(store, r) {
        Ok(p) => p,
        Err(code) => {
            out.auth = Err(format!("{}: {code}", tr("password", "密码")));
            return out;
        }
    };
    let runtime = match runtime() {
        Ok(rt) => rt,
        Err(code) => {
            out.auth = Err(code);
            return out;
        }
    };
    use oo_connectors::engine::SourceEngine;
    match r.engine {
        Engine::Postgres => {
            let result = pg_engine(store, r, password).and_then(|mut engine| {
                let version = runtime
                    .block_on(engine.probe())
                    .map(|p| p.version.raw)
                    .map_err(|e| e.to_string())?;
                // The database list is a convenience: sign-in stands without it.
                let databases = runtime
                    .block_on(engine.list_databases(&wizard_budget()))
                    .unwrap_or_default();
                Ok((version, databases))
            });
            match result {
                Ok((version, databases)) => {
                    out.auth = Ok(String::new());
                    out.checked = true;
                    out.version = Some(version);
                    out.databases = databases;
                }
                Err(e) => out.auth = Err(e),
            }
        }
        Engine::Oracle => {
            if std::env::var_os("OO_ORACLE_WORKER_IMAGE").is_some() {
                out.auth = Ok(tr(
                    "sign-in is checked by oo doctor through the Oracle worker",
                    "登录由 oo doctor 通过 Oracle worker 检查",
                )
                .into());
                return out;
            }
            let result = oracle_engine(r, password).and_then(|mut engine| {
                let version = runtime.block_on(engine.probe())?.version.raw;
                let services = engine.list_services(&wizard_budget()).unwrap_or_default();
                Ok((version, services))
            });
            match result {
                Ok((version, services)) => {
                    out.auth = Ok(String::new());
                    out.checked = true;
                    out.version = Some(version);
                    out.databases = services;
                }
                Err(e) => {
                    let text = e.to_string();
                    // No Oracle client libraries on this machine: not a
                    // sign-in failure; the doctor checks through its worker.
                    if text.contains("DPI-1047") || text.contains("Cannot locate") {
                        out.auth = Ok(tr(
                            "Oracle client not installed here; oo doctor checks sign-in next",
                            "本机没有 Oracle 客户端；登录由 oo doctor 下一步检查",
                        )
                        .into());
                    } else {
                        out.auth = Err(text);
                    }
                }
            }
        }
        Engine::Mysql => {
            out.auth = Ok(tr(
                "sign-in is checked by oo doctor next (MySQL helper)",
                "登录由 oo doctor 下一步检查（MySQL helper）",
            )
            .into());
        }
    }
    out
}

/// Screen 2's table list from the source catalog (statistics only, no
/// rows). `Ok(None)`: this engine has no local discovery path here.
fn discover(
    store: &SecretStore,
    r: &connect::Request,
) -> Result<Option<Vec<crate::tui::scorer::TableMeta>>, String> {
    let found = match r.engine {
        Engine::Postgres => {
            let rt = runtime()?;
            let mut engine = pg_engine(store, r, request_password(store, r)?)?;
            rt.block_on(engine.discover_catalog(&wizard_budget()))
                .map_err(|e| e.to_string())?
        }
        Engine::Oracle if std::env::var_os("OO_ORACLE_WORKER_IMAGE").is_none() => {
            oracle_engine(r, request_password(store, r)?)
                .and_then(|mut engine| engine.discover_catalog(&wizard_budget()))
                .map_err(|e| e.to_string())?
        }
        _ => return Ok(None),
    };
    Ok(Some(table_metas(found)))
}

fn table_metas(
    found: Vec<oo_connectors::engine::DiscoveredTable>,
) -> Vec<crate::tui::scorer::TableMeta> {
    found
        .into_iter()
        .map(|t| crate::tui::scorer::TableMeta {
            schema: t.table.schema,
            name: t.table.name,
            rows: t.estimated_rows,
            has_primary_key: t.has_primary_key,
            has_unique_key: t.has_unique_key,
            partition_child: t.partition_child,
            references: t
                .references
                .into_iter()
                .map(|r| format!("{}.{}", r.schema, r.name))
                .collect(),
        })
        .collect()
}

/// Non-TTY screen 1: what the flags say, then the same live checks
/// (resolve, port, TLS certificate, sign-in when the password is a
/// reference). Writes nothing. `oo.init-connect/v1` with `--json`.
fn connect_check(opts: &InitOptions) -> Result<Invocation, Failure> {
    use crate::tui::netprobe;
    let p = opts.connect.prefill()?;
    let engine = p
        .engine
        .ok_or_else(|| Failure::usage("ENGINE_REQUIRED: pass --engine postgres|mysql|oracle"))?;
    if p.host.is_empty() {
        return Err(Failure::usage("HOST_REQUIRED: pass --host or --dsn"));
    }
    if p.password.is_some() {
        return Err(Failure::usage(
            "INLINE_PASSWORD: use --password-env VAR or --password-file PATH",
        ));
    }
    let port = p.port_or_default().unwrap_or(engine.port());
    let mut checks: Vec<Value> = Vec::new();
    let mut check = |name: &str, ok: bool, detail: String| {
        checks.push(json!({"name":name,"status":if ok {"ok"} else {"blocked"},"detail":detail}));
        ok
    };
    let mut version = opts.connect.version.clone();
    let mut verified = None;
    let resolved = netprobe::resolve(&p.host, port);
    let reached = check(
        "resolve",
        resolved.is_ok(),
        match &resolved {
            Ok(ips) => ips
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
            Err(e) => (*e).into(),
        },
    ) && {
        let open = netprobe::port_open(&p.host, port);
        check(
            "port",
            open.is_ok(),
            match open {
                Ok(d) => format!("{} ms", d.as_millis()),
                Err(e) => e.into(),
            },
        )
    };
    let mut ca_pem = None;
    if reached && engine.uses_ca() {
        match netprobe::tls_chain(engine, &p.host, port) {
            Ok(report) => {
                if let Some(v) = &report.server_version {
                    version.get_or_insert(v.clone());
                }
                let leaf = report.chain.first();
                check(
                    "tls",
                    leaf.is_some(),
                    leaf.map_or(String::new(), |c| {
                        format!(
                            "{} · issuer {} · until {} · sha256 {}",
                            c.subject, c.issuer, c.not_after, c.sha256
                        )
                    }),
                );
            }
            Err(e) => {
                check("tls", false, e.into());
            }
        }
        if !p.ca_file.is_empty() {
            ca_pem = std::fs::read_to_string(&p.ca_file)
                .ok()
                .filter(|t| t.contains("BEGIN CERTIFICATE"));
            check(
                "ca_file",
                ca_pem.is_some(),
                if ca_pem.is_some() {
                    p.ca_file.clone()
                } else {
                    "TUI_CA_PEM_REQUIRED".into()
                },
            );
        }
    }
    if reached
        && let Some(reference) = &p.password_ref
        && (ca_pem.is_some() || !engine.uses_ca())
    {
        let store = SecretStore::from_home()
            .or_else(|_| SecretStore::open(std::env::temp_dir().join("oo-no-store")));
        if let Ok(store) = store {
            let v = verify(
                &store,
                &connect::Request {
                    engine,
                    host: p.host.clone(),
                    port,
                    database: if p.database.is_empty() {
                        engine.default_database().into()
                    } else {
                        p.database.clone()
                    },
                    user: if p.user.is_empty() {
                        engine.suggested_user().into()
                    } else {
                        p.user.clone()
                    },
                    secret: connect::Secret::Ref(reference.clone()),
                    ca_pem: ca_pem.clone(),
                    ca_ref: None,
                    generation: 0,
                },
            );
            check(
                "sign_in",
                v.auth.is_ok(),
                match &v.auth {
                    Ok(note) if v.checked => "ok".into(),
                    Ok(note) => note.clone(),
                    Err(e) => e.clone(),
                },
            );
            if let Some(raw) = &v.version {
                version = Some(raw.clone());
            }
            verified = Some(v.checked);
        }
    }
    let label = version
        .as_deref()
        .and_then(|raw| {
            src::match_version(engine, raw)
                .or_else(|| src::versions_of(engine).iter().position(|v| v.label == raw))
        })
        .map(|i| src::versions_of(engine)[i].clone());
    let blocked = checks.iter().any(|c| c["status"] != "ok");
    let doc = json!({
        "protocol":"oo.init-connect/v1",
        "understood":{"engine":engine.id(),"host":p.host,"port":port,
            "database":if p.database.is_empty() {engine.default_database().to_owned()} else {p.database.clone()},
            "sid":p.sid,"user":if p.user.is_empty() {engine.suggested_user().to_owned()} else {p.user.clone()},
            "password_ref":p.password_ref,"ca_file":p.ca_file},
        "version":{"reported":version,"label":label.as_ref().map(|v| &v.label),
            "full":label.as_ref().map(|v| &v.full),"cdc":label.as_ref().map(|v| &v.cdc)},
        "signed_in":verified,
        "checks":checks,
        "written":false,
    });
    let exit = u8::from(blocked);
    if opts.json {
        return Ok(Invocation::AdminReport(doc.to_string(), exit));
    }
    let mut text = format!(
        "{} {}:{} / {} as {}\n",
        engine.name(),
        doc["understood"]["host"].as_str().unwrap_or(""),
        port,
        doc["understood"]["database"].as_str().unwrap_or(""),
        doc["understood"]["user"].as_str().unwrap_or("")
    );
    for c in doc["checks"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "{} {:<8} {}\n",
            if c["status"] == "ok" { "✓" } else { "✗" },
            c["name"].as_str().unwrap_or(""),
            c["detail"].as_str().unwrap_or("")
        ));
    }
    if let Some(v) = &label {
        text.push_str(&format!(
            "version {} · {} · CDC {}\n",
            v.label, v.full, v.cdc
        ));
    }
    Ok(Invocation::AdminReport(text.trim_end().to_owned(), exit))
}

/// Doctor report (`oo.doctor/v1`) as checklist rows.
pub(super) fn doctor_checks(report: &Value) -> Vec<Check> {
    let status = |v: &Value| match v.as_str() {
        Some("ok") => Status::Pass,
        Some("blocked" | "failed") => Status::Fail,
        Some("warn" | "degraded") => Status::Warn,
        _ => Status::Waiting,
    };
    let text = |v: &Value, keys: &[&str]| {
        keys.iter()
            .find_map(|k| match &v[*k] {
                Value::String(s) => Some(s.clone()),
                Value::Array(a) => a.first().and_then(Value::as_str).map(str::to_owned),
                _ => None,
            })
            .unwrap_or_default()
    };
    let mut checks = Vec::new();
    for source in report["sources"].as_array().into_iter().flatten() {
        let id = source["source_id"].as_str().unwrap_or("source");
        checks.push(Check::new(
            &format!("source {id}"),
            status(&source["status"]),
            &text(source, &["error", "scope_error"]),
        ));
        if let Some(fields) = source.as_object() {
            for (name, value) in fields {
                if value.get("status").is_some_and(Value::is_string) {
                    checks.push(
                        Check::new(
                            &name.replace('_', " "),
                            status(&value["status"]),
                            &text(value, &["error", "detail"]),
                        )
                        .fix(&text(value, &["dba_statements", "fix", "advice"])),
                    );
                }
            }
        }
    }
    checks.push(Check::new(
        "model probe",
        status(&report["model"]["status"]),
        &text(&report["model"], &["error"]),
    ));
    checks
}

fn show_checks(screen: &mut Screen, endpoint: &str, checks: Vec<Check>) -> Result<(), Failure> {
    let mut state = connect::report(endpoint, checks);
    loop {
        screen
            .term()
            .draw(|area, buf, theme| connect::render(&state, area, buf, theme))
            .map_err(Failure::local)?;
        if let Some(key) = screen
            .term()
            .key(std::time::Duration::from_millis(100))
            .map_err(Failure::local)?
        {
            match connect::key(&mut state, key) {
                crate::tui::screens::Action::Next => return Ok(()),
                crate::tui::screens::Action::Quit => return Err(Failure::local("TUI_CANCELLED")),
                crate::tui::screens::Action::None => (),
            }
        }
        state.frame += 1;
    }
}

/// `oo quickstart` / `oo init --demo` on a TTY: every screen on the bundled
/// demo catalog and adjudication. The demo runtime and pack are V3-M-01; this
/// preview starts nothing and writes nothing.
fn demo(opts: &InitOptions, theme: Theme, splash: bool) -> Result<Invocation, Failure> {
    let mut term = Term::open(theme).map_err(Failure::local)?;
    if splash {
        app::splash(&mut term).map_err(Failure::local)?;
    }
    // The demo needs no connection details: screen 1 is skipped.
    let tables = catalog_tables(&InitOptions {
        demo: true,
        catalog: opts.catalog.clone(),
        ..InitOptions::default()
    })?
    .unwrap_or_default();
    let Some(selected) = app::select(&mut term, &tables).map_err(Failure::local)? else {
        return Err(Failure::local("TUI_CANCELLED"));
    };
    // This path still previews shipped assets. Do not invent successful build stages.
    let done = json!({"stages":[]});
    let mut first = true;
    app::build(&mut term, &mut || {
        (std::mem::take(&mut first).then(|| done.clone()), true)
    })
    .map_err(Failure::local)?;
    let adj = crate::tui::adjudication::parse(
        serde_json::from_str(include_str!(
            "../../../../deploy/lite/demo/adjudication.json"
        ))
        .map_err(|_| Failure::local("DEMO_ADJUDICATION"))?,
    )
    .map_err(Failure::local)?;
    let Some((confirm, overrides)) =
        app::confirm(&mut term, adj, &crate::tui::adjudication::now_rfc3339())
            .map_err(Failure::local)?
    else {
        return Err(Failure::local("TUI_CANCELLED"));
    };
    let profile = opts.profile.clone().unwrap_or_else(|| "demo".into());
    let delivery = crate::tui::backend::Local {
        root: crate::lifecycle::directory().map_err(|_| Failure::local("PROFILE_DIRECTORY"))?,
        issued_profile: None,
        zh: zh(),
    };
    app::deliver(&mut term, &delivery, &profile).map_err(Failure::local)?;
    drop(term);
    Ok(Invocation::Help(
        json!({"protocol":"oo.quickstart-preview/v1","selected":selected,
            "publish_confirm":confirm,"overrides":overrides.items.len(),"written":false,
            "note":"preview only: the demo runtime and pack are started by V3-M-01"})
        .to_string(),
    ))
}

pub(super) fn run(opts: &InitOptions) -> Result<Invocation, Failure> {
    let theme = Theme::from_env(opts.no_anim);
    if opts.demo {
        return demo(opts, theme, true);
    }
    if Path::new("stack.yaml").symlink_metadata().is_ok() {
        return Err(Failure::local("TUI_CONFIG_EXISTS_OR_IO"));
    }
    let tables = catalog_tables(opts)?;
    let home = std::env::var_os("HOME").ok_or_else(|| Failure::local("SECRET_HOME"))?;
    let parent = Path::new(&home).join(".config/open-ontology");
    fs::create_dir_all(&parent).map_err(|_| Failure::local("SECRET_DIRECTORY"))?;
    let store = SecretStore::create(parent.join("secrets")).map_err(Failure::local)?;
    let mut screen = Screen::with(theme)?;
    app::splash(screen.term()).map_err(Failure::local)?;
    let profile = opts.profile.clone().unwrap_or_else(|| "default".into());
    let vars: std::collections::BTreeMap<String, String> = std::env::vars()
        .filter(|(k, _)| {
            k.starts_with("PG")
                || k.starts_with("MYSQL_")
                || k.starts_with("ORACLE_")
                || k == "TNS_ADMIN"
        })
        .collect();
    let home_path = Path::new(&home).to_path_buf();
    let mut start = connect::Connect::new(src::discover(&src::Machine {
        home: Some(&home_path),
        vars: &vars,
        profile: &profile,
    }));
    start.home = Some(home_path.clone());
    if opts.connect.any() {
        let prefill = opts.connect.prefill()?;
        let version = opts.connect.version.clone().unwrap_or_default();
        start.open(
            &Prefill {
                engine: prefill.engine.or(Some(Engine::Postgres)),
                ..prefill
            },
            &version,
        );
    }
    let live = WizardLive { store: &store };
    let Some(mut conn) = app::connect(screen.term(), start, &live).map_err(Failure::local)? else {
        return Err(Failure::local("TUI_CANCELLED"));
    };
    if conn.demo {
        drop(screen);
        return demo(opts, theme, false);
    }
    let masked = connect::endpoint(&conn);
    // Screen 2's list comes from the server unless --catalog supplied one.
    let mut discovery_note = None;
    let tables = match tables {
        Some(tables) => Some(tables),
        None => match conn.request().map(|r| discover(&store, &r)) {
            Some(Ok(found)) => found,
            Some(Err(reason)) => {
                discovery_note = Some(reason);
                None
            }
            None => None,
        },
    };
    let password = match conn.password_ref() {
        Some(reference) => json!({"secret_ref": reference}),
        None => {
            let typed = conn
                .take_password()
                .ok_or_else(|| Failure::usage("TUI_PASSWORD_REQUIRED"))?;
            store_new(&store, "source-password", typed.as_bytes())?
        }
    };
    let engine = conn.engine;
    let mut source = json!({
        "id": "source", "engine": engine.id(), "host": conn.host.trim(),
        "port": conn.port_number().ok_or_else(|| Failure::usage("DSN_PORT"))?,
        "database": conn.database.trim(), "user": conn.user.trim(), "password": password,
    });
    let mut ca_ref = String::new();
    if engine.uses_ca() {
        let ca = match conn.tls_mode {
            connect::TlsMode::Saved => json!({"secret_ref": conn.ca_saved}),
            _ => {
                let pem = conn
                    .ca_pem()
                    .ok_or_else(|| Failure::usage("TUI_CA_FILE_REQUIRED"))?;
                oo_connectors::PgTlsConfig::from_ca_pem(pem.as_bytes())
                    .map_err(|_| Failure::usage("TUI_CA_PEM_REQUIRED"))?;
                store_new(&store, "source-ca", pem.as_bytes())?
            }
        };
        ca_ref = ca["secret_ref"].as_str().unwrap_or("").to_owned();
        source["tls_ca"] = ca;
    }
    // Remember the connection (no secret values) for the next run.
    let _ = src::remember(
        &home_path,
        &src::Recent {
            profile: profile.clone(),
            engine,
            host: conn.host.trim().to_owned(),
            port: conn.port_number().unwrap_or(engine.port()),
            database: conn.database.trim().to_owned(),
            sid: conn.sid,
            user: conn.user.trim().to_owned(),
            version: connect::version_label(&conn),
            password_ref: source["password"]["secret_ref"].as_str().map(str::to_owned),
            ca_file: if conn.tls_mode == connect::TlsMode::File {
                conn.ca_file.trim().to_owned()
            } else {
                String::new()
            },
            ca_ref,
        },
    );
    if let Some(ssh) = source_ssh(&mut screen, &store)? {
        source["ssh"] = ssh;
    }
    let include = match &tables {
        Some(tables) => app::select(screen.term(), tables)
            .map_err(Failure::local)?
            .ok_or_else(|| Failure::local("TUI_CANCELLED"))?,
        None => {
            let why = match &discovery_note {
                Some(reason) => format!("Could not list tables: {reason}"),
                None => "Table discovery for this engine runs in oo doctor.".to_owned(),
            };
            list(&screen.prompt(
                2,
                &format!("{why}\nInclude schema.table entries, comma separated (or rerun with --catalog CATALOG.json to choose from a list)"),
                "",
                false,
            )?)
        }
    };
    let exclude = if tables.is_some() {
        Vec::new()
    } else {
        list(&screen.prompt(2, "Exclude tables or schema.*, comma separated", "", false)?)
    };
    source["scope"] = json!({"include":include,"exclude":exclude});
    let provider = screen.prompt(
        3,
        "Provider: openai-compatible / anthropic",
        "openai-compatible",
        false,
    )?;
    let url = screen.prompt(
        3,
        "Model base URL, or @PATH to import a private endpoint file",
        "",
        false,
    )?;
    let base_url = match url.strip_prefix('@') {
        Some(path) => {
            let name = screen.prompt(
                3,
                "Endpoint secret name (new file)",
                "model-endpoint",
                false,
            )?;
            if !oo_stack::stack::secret_name(&name) {
                return Err(Failure::usage("SECRET_NAME"));
            }
            import_file(&store, &name, Path::new(path), false)?
        }
        None => json!(url),
    };
    let model = json!({
        "provider":provider,
        "base_url":base_url,
        "model":screen.prompt(3, "Model ID", "", false)?,
        "key":secret_or_import(&mut screen, &store, 3, "Model key", "model-key")?,
        "context_window":"auto",
    });
    screen.prompt(4, "Deployment: local Docker / lite. Remote SSH deployment is not yet available. Enter to continue.", "", false)?;
    let stack = oo_stack::parse(
        &json!({
            "version":"oo.stack/v1", "sources":[source], "model":model,
            "deploy":{"target":"local","profile":"lite"},
            "gateway":{"public_url":"auto"},
        })
        .to_string(),
    )
    .map_err(Failure::usage)?;
    // Read-only doctor and the bounded model probe, shown as a checklist.
    let preflight = doctor::run(&stack, true)?;
    let (report, exit) = match preflight {
        Invocation::AdminReport(report, exit) => (report, exit),
        _ => return Err(Failure::local("TUI_PREFLIGHT_PROTOCOL")),
    };
    let report: Value =
        serde_json::from_str(&report).map_err(|_| Failure::local("TUI_PREFLIGHT_PROTOCOL"))?;
    show_checks(&mut screen, &masked, doctor_checks(&report))?;
    let confirm = screen.prompt(6,
        "Type SAVE for configuration only, or BUILD to run oo up and oo build: deploy, load, Self-build, owner review, publication, Gateway and share prompt. BUILD requires passing preflight. Credentials remain saved on cancellation.",
        "", false)?;
    if confirm != "SAVE" && confirm != "BUILD" {
        return Err(Failure::local("TUI_CANCELLED"));
    }
    save_new(&stack, Path::new("stack.yaml"))?;
    if confirm == "BUILD" {
        drop(screen);
        return build_saved("stack.yaml", exit);
    }
    screen.prompt(
        7,
        &format!(
            "Configuration saved (0600).\n{}\nEnter to exit.",
            super::progress::completion(&json!({"status":"not_started"}))
        ),
        "",
        false,
    )?;
    Ok(Invocation::Help(
        "Stack saved; no services started or publication performed.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_action_uses_the_cli_entry_point_and_refuses_failed_preflight() {
        let path = "nonexistent-wizard-stack.yaml";
        let wizard = build_saved(path, 0).err().unwrap().json_bytes();
        let cli = super::super::execute("build", &["--config".into(), path.into()])
            .err()
            .unwrap()
            .json_bytes();
        assert_eq!(wizard, cli);
        let blocked = build_saved(path, 1).err().unwrap().json_bytes();
        assert!(
            String::from_utf8(blocked)
                .unwrap()
                .contains("TUI_PREFLIGHT_BLOCKED")
        );
    }

    #[test]
    fn imports_bounded_ca_into_new_private_secret_without_overwriting() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = // A private temp dir, not the repository evidence tree.
        std::fs::canonicalize(std::env::temp_dir()).unwrap()
            .join(format!("wizard-import-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let store = SecretStore::create(root.join("secrets")).unwrap();
        let ca = root.join("ca.pem");
        let pem = b"-----BEGIN CERTIFICATE-----\nfixture\n-----END CERTIFICATE-----\n";
        fs::write(&ca, pem).unwrap();
        assert!(import_file(&store, "source-ca", &ca, true).is_ok());
        assert_eq!(store.read("source-ca").unwrap().as_bytes(), pem);
        assert_eq!(
            fs::metadata(root.join("secrets/source-ca"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(import_file(&store, "source-ca", &ca, true).is_err());
        fs::write(&ca, b"not a certificate").unwrap();
        assert!(import_file(&store, "invalid", &ca, true).is_err());
        fs::write(&ca, vec![b'x'; 65_537]).unwrap();
        assert!(import_file(&store, "oversized", &ca, false).is_err());
        symlink(&ca, root.join("link")).unwrap();
        assert!(import_file(&store, "link", &root.join("link"), false).is_err());
        assert!(import_file(&store, "directory", &root, false).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pin_requires_full_out_of_band_confirmation() {
        let pin = format!("SHA256:{}", "A".repeat(43));
        assert_eq!(confirmed_pin(&pin, &pin).ok(), Some(pin.clone()));
        for answer in ["", "yes", "SHA256:changed"] {
            assert!(confirmed_pin(&pin, answer).is_err());
        }
        assert!(confirmed_pin("bad", "bad").is_err());
    }

    #[test]
    fn config_bytes_match_noninteractive_normalization_and_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("oo-wizard-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let config = oo_stack::parse(include_str!("../../../oo-stack/example.stack.json")).unwrap();
        let wizard = dir.join("wizard");
        let noninteractive = dir.join("noninteractive");
        assert!(save_new(&config, &wizard).is_ok());
        fs::write(
            &noninteractive,
            include_str!("../../../oo-stack/example.stack.json"),
        )
        .unwrap();
        assert!(super::super::normalize(&noninteractive).is_ok());
        assert_eq!(
            fs::read(&wizard).unwrap(),
            fs::read(&noninteractive).unwrap()
        );
        assert!(save_new(&config, &wizard).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
