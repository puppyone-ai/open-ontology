//! `oo init`: connect, choose tables, then the remaining configuration
//! questions, preflight and an optional build. A saved configuration is not a
//! deployed/reviewed pack. Screens and their visual language live in
//! `crate::tui`; this module supplies the real probes and side effects.
use std::fs::{self, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::tui::app::{self, ConnectRequest, Term};
use crate::tui::screens::{Check, Status, connect};
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
            _ => return Err(bad()),
        }
        index += 1;
    }
    if opts.tables.is_some() && opts.tables_recommended {
        return Err(Failure::usage(
            "ADMIN_ARGUMENT: --tables and --tables-recommended are exclusive",
        ));
    }
    Ok(opts)
}

fn zh() -> bool {
    ["OO_LANG", "LC_ALL", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .is_some_and(|v| v.starts_with("zh"))
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

/// Screen 1 probe for a real database: engine, TCP reachability and TLS
/// material. Account, CDC and catalog checks need the table list and run in
/// `oo doctor` after screen 2. Nothing here authenticates or writes.
fn probe(request: &ConnectRequest) -> Vec<Check> {
    if request.demo {
        return demo_checks();
    }
    let Some(dsn) = &request.dsn else {
        return vec![
            Check::new("connection string", Status::Fail, "incomplete")
                .fix("scheme://user@host:port/database"),
        ];
    };
    let mut checks = vec![Check::new(
        "engine",
        Status::Pass,
        &format!("{} (from your input)", dsn.engine),
    )];
    let started = std::time::Instant::now();
    let reached = std::net::ToSocketAddrs::to_socket_addrs(&(dsn.host.as_str(), dsn.port))
        .ok()
        .and_then(|mut addrs| addrs.next())
        .and_then(|addr| {
            std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3)).ok()
        });
    checks.push(match reached {
        Some(_) => Check::new(
            "reachable",
            Status::Pass,
            &format!(
                "{}:{} answered in {} ms",
                dsn.host,
                dsn.port,
                started.elapsed().as_millis()
            ),
        ),
        None => Check::new(
            "reachable",
            Status::Warn,
            &format!("{}:{} did not answer", dsn.host, dsn.port),
        )
        .fix("check host, port and firewall; an SSH jump host can be added next"),
    });
    let needs_ca = matches!(dsn.engine.as_str(), "postgres" | "mysql" | "kingbase");
    checks.push(match (&request.ca, needs_ca) {
        (Some(_), _) => Check::new("TLS", Status::Pass, "CA file given; verified by oo doctor"),
        (None, true) => Check::new("TLS", Status::Warn, "a CA file is required for this engine")
            .fix("you will be asked for the CA PEM path next"),
        (None, false) => Check::new("TLS", Status::Pass, "engine-native TLS settings"),
    });
    for name in ["read-only account", "CDC capability", "catalog access"] {
        checks.push(Check::new(
            name,
            Status::Waiting,
            "checked by oo doctor after you choose tables",
        ));
    }
    checks
}

fn demo_checks() -> Vec<Check> {
    vec![
        Check::new(
            "demo catalog",
            Status::Pass,
            "OO-Logistics subset bundled with oo",
        ),
        Check::new(
            "demo runtime",
            Status::Waiting,
            "started by oo quickstart (V3-M-01); not by this preview",
        ),
    ]
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

fn show_checks(screen: &mut Screen, dsn_masked: &str, checks: Vec<Check>) -> Result<(), Failure> {
    let mut state = connect::Connect::new();
    state.dsn = dsn_masked.to_owned();
    state.checks = checks;
    state
        .checks
        .push(Check::new("next", Status::Waiting, "press ⏎ to continue"));
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
            match key.code {
                crossterm::event::KeyCode::Enter => return Ok(()),
                crossterm::event::KeyCode::Esc => return Err(Failure::local("TUI_CANCELLED")),
                _ => (),
            }
        }
        state.frame += 1;
    }
}

/// `oo quickstart` / `oo init --demo` on a TTY: every screen on the bundled
/// demo catalog and adjudication. The demo runtime and pack are V3-M-01; this
/// preview starts nothing and writes nothing.
fn demo(opts: &InitOptions, theme: Theme) -> Result<Invocation, Failure> {
    let mut term = Term::open(theme).map_err(Failure::local)?;
    app::splash(&mut term).map_err(Failure::local)?;
    let mut start = connect::Connect::new();
    start.mode = 2;
    let Some(_) = app::connect(&mut term, start, &probe).map_err(Failure::local)? else {
        return Err(Failure::local("TUI_CANCELLED"));
    };
    let tables = catalog_tables(&InitOptions {
        demo: true,
        catalog: opts.catalog.clone(),
        ..InitOptions::default()
    })?
    .unwrap_or_default();
    let Some(selected) = app::select(&mut term, &tables, zh()).map_err(Failure::local)? else {
        return Err(Failure::local("TUI_CANCELLED"));
    };
    let done = json!({"stages":[
        {"name":"load","status":"done"},{"name":"profile","status":"done"},{"name":"draft","status":"done"},
        {"name":"adjudicate","status":"done"},{"name":"compile","status":"done"}]});
    let mut first = true;
    app::build(&mut term, &mut || {
        (std::mem::take(&mut first).then(|| done.clone()), true)
    })
    .map_err(Failure::local)?;
    let adj = crate::tui::adjudication::parse(
        serde_json::from_str(include_str!("../../tests/fixtures/adjudication.json"))
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
    app::deliver(&mut term, &crate::tui::backend::Pending, &profile).map_err(Failure::local)?;
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
        return demo(opts, theme);
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
    let Some(conn) =
        app::connect(screen.term(), connect::Connect::new(), &probe).map_err(Failure::local)?
    else {
        return Err(Failure::local("TUI_CANCELLED"));
    };
    if conn.mode == 2 {
        drop(screen);
        return demo(opts, theme);
    }
    let dsn = conn.dsn().map_err(Failure::usage)?;
    let masked = if conn.mode == 0 {
        connect::mask(&conn.dsn)
    } else {
        format!(
            "{}://{}@{}:{}/{}",
            dsn.engine, dsn.user, dsn.host, dsn.port, dsn.database
        )
    };
    let user = if dsn.user.is_empty() {
        screen.prompt(1, "Read-only user", "", false)?
    } else {
        dsn.user.clone()
    };
    let database = if dsn.database.is_empty() {
        screen.prompt(1, "Database or Oracle service", "", false)?
    } else {
        dsn.database.clone()
    };
    let password = match &dsn.password {
        Some(value) => store_new(&store, "source-password", value.as_bytes())?,
        None => secret(&mut screen, &store, 1, "Source password", "source-password")?,
    };
    let mut source = json!({
        "id": "source", "engine": dsn.engine, "host": dsn.host, "port": dsn.port,
        "database": database, "user": user, "password": password,
    });
    let ca = match conn.ca_path() {
        Some(path) => path.to_owned(),
        None => screen.prompt(
            1,
            "TLS CA PEM file path (imported into secret store; required for PostgreSQL/MySQL)",
            "",
            false,
        )?,
    };
    if !ca.is_empty() {
        let name = screen.prompt(1, "TLS CA secret name (new file)", "source-ca", false)?;
        source["tls_ca"] = import_file(&store, &name, Path::new(&ca), true)?;
    } else if matches!(dsn.engine.as_str(), "postgres" | "mysql") {
        return Err(Failure::usage("TUI_CA_FILE_REQUIRED"));
    }
    if let Some(ssh) = source_ssh(&mut screen, &store)? {
        source["ssh"] = ssh;
    }
    let include = match &tables {
        Some(tables) => app::select(screen.term(), tables, zh())
            .map_err(Failure::local)?
            .ok_or_else(|| Failure::local("TUI_CANCELLED"))?,
        None => list(&screen.prompt(2, "Table discovery is not available for this engine yet.\nInclude schema.table entries, comma separated (or rerun with --catalog CATALOG.json to choose from a list)", "", false)?),
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
