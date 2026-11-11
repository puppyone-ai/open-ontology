//! Screen 1: connect. Choose instead of type.
//!
//! The first view is one list: connections this machine already knows
//! (recent ones, `~/.pgpass`, `~/.my.cnf`, `tnsnames.ora`, `PG*` / `MYSQL_*`
//! variables), then the supported engines with their version range and
//! support level, then the demo. Anything typed or pasted (URL, JDBC URL,
//! `.env` lines, `host:port`, a TNS descriptor) is parsed as it arrives and
//! fills the form.
//!
//! The form is prefilled with defaults (port, suggested read-only user,
//! common Oracle services); each field is checked live (host resolves, port
//! open, the TLS certificate the server presents) and ⏎ signs in, detects
//! the version and preselects it in the version picker.
//!
//! Secrets: a typed password lives only in `password`; it, its length and
//! any pasted password never reach a render buffer.
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Span;

use super::{Action, Check, checklist, input, put};
use crate::tui::fx;
use crate::tui::netprobe::{CertInfo, TlsReport};
use crate::tui::source::{self, Engine, Found, MASK, Prefill, Version};
use crate::tui::theme::{self, Hint, Theme, Tone, fit, heading, width};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// One list: known connections, engines, demo; plus the paste line.
    Pick,
    /// Prefilled fields with live checks.
    Form,
    /// The doctor checklist after the source is saved.
    Report,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Version,
    Host,
    Port,
    Database,
    ConnectBy,
    User,
    Password,
    Tls,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassMode {
    /// Typed, masked; stored in the secret store.
    Type,
    /// `env:VAR` reference; never copied.
    Env,
    /// `file:/abs/path` reference; never copied.
    File,
    /// Already in the secret store from an earlier run.
    Saved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsMode {
    /// Not decided: waiting for the server's certificate.
    Auto,
    /// Trust the CA the server presented (loopback / private network only).
    Trust,
    /// A CA bundle file.
    File,
    /// A CA saved in the secret store by an earlier run.
    Saved,
}

/// A row of the pick list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Found(usize),
    Engine(Engine),
    Demo,
}

/// The secret part of a sign-in request.
#[derive(Clone)]
pub enum Secret {
    Typed(String),
    /// `env:VAR` or `file:/abs/path`.
    Ref(String),
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Secret::Typed(_) => f.write_str("Secret::Typed([redacted])"),
            Secret::Ref(r) => write!(f, "Secret::Ref({r})"),
        }
    }
}

/// What the runner signs in with. `generation` ties the answer to the
/// form state it was asked for.
#[derive(Clone, Debug)]
pub struct Request {
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    /// Database, or Oracle service (a SID is tried as a service name).
    pub database: String,
    pub user: String,
    pub secret: Secret,
    /// PEM trust anchor for PostgreSQL / MySQL.
    pub ca_pem: Option<String>,
    /// Or a CA already in the secret store.
    pub ca_ref: Option<String>,
    pub generation: u64,
}

/// Answer to a sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    pub generation: u64,
    /// `Ok(note)` signed in (or deferred with a note); `Err(reason)`.
    pub auth: Result<String, String>,
    /// `true` when sign-in really ran (not deferred to the doctor).
    pub checked: bool,
    /// Server-reported version.
    pub version: Option<String>,
    /// Databases / services the server listed, if the engine can.
    pub databases: Vec<String>,
}

/// Background work the screen wants.
#[derive(Clone, Debug)]
pub enum Job {
    Resolve(String),
    Port(String, u16),
    Tls(Engine, String, u16),
    Verify(Request),
}

impl Job {
    /// Dedup key; never contains the secret.
    pub fn key(&self) -> String {
        match self {
            Job::Resolve(h) => format!("resolve {h}"),
            Job::Port(h, p) => format!("port {h}:{p}"),
            Job::Tls(_, h, p) => format!("tls {h}:{p}"),
            Job::Verify(r) => format!("verify {}", r.generation),
        }
    }
}

/// A finished job.
#[derive(Clone, Debug)]
pub enum Outcome {
    Resolved(String, Result<Vec<IpAddr>, &'static str>),
    Port(String, u16, Result<Duration, &'static str>),
    Tls(String, u16, Result<TlsReport, &'static str>),
    Verified(Verified),
}

impl Outcome {
    /// The key of the job that produced it.
    pub fn key(&self) -> String {
        match self {
            Outcome::Resolved(h, _) => format!("resolve {h}"),
            Outcome::Port(h, p, _) => format!("port {h}:{p}"),
            Outcome::Tls(h, p, _) => format!("tls {h}:{p}"),
            Outcome::Verified(v) => format!("verify {}", v.generation),
        }
    }
}

/// A live answer for one `(host, port)`.
pub type Answer<T> = Option<((String, u16), Result<T, &'static str>)>;

#[derive(Clone, Debug, Default)]
pub struct Live {
    pub resolved: Option<(String, Result<Vec<IpAddr>, &'static str>)>,
    pub port: Answer<Duration>,
    pub tls: Answer<TlsReport>,
    pub verified: Option<Verified>,
    pub verifying: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Connect {
    pub stage: Stage,
    pub found: Vec<Found>,
    pub cursor: usize,
    /// Pasted / typed text on the pick view. May hold a password: rendered
    /// only through `source::mask`.
    pub paste: String,
    /// The user chose the demo.
    pub demo: bool,
    pub engine: Engine,
    /// Picked version (index into `versions_of(engine)`); `None` = detect.
    pub version: Option<usize>,
    pub host: String,
    pub port: String,
    pub database: String,
    pub sid: bool,
    pub user: String,
    pub pass_mode: PassMode,
    /// Typed password. Never rendered.
    password: String,
    pub pass_env: String,
    pub pass_file: String,
    /// Secret-store name of a saved password.
    pub pass_saved: String,
    pub tls_mode: TlsMode,
    pub ca_file: String,
    /// Secret-store name of a saved CA.
    pub ca_saved: String,
    pub field: Field,
    /// Fields still holding a suggested default: dim, replaced by the first
    /// character typed.
    pub defaulted: Vec<Field>,
    /// Database / service candidates for ←→.
    pub db_options: Vec<String>,
    /// Tab-completion candidates for the focused path field.
    pub completions: Vec<String>,
    pub live: Live,
    /// Bumped on every edit that changes what sign-in would use.
    pub generation: u64,
    /// Frame of the last edit, for debouncing live checks.
    pub edited_at: usize,
    pub want_verify: bool,
    pub frame: usize,
    pub error: Option<(&'static str, &'static str)>,
    /// Doctor checklist (Report stage) and its masked endpoint line.
    pub checks: Vec<Check>,
    pub checks_at: usize,
    pub endpoint: String,
    /// For `~/` in path completion.
    pub home: Option<PathBuf>,
    /// Whether `env:` names resolve (set by the runner, read by render).
    pub env_set: bool,
}

impl Default for Connect {
    fn default() -> Self {
        Connect::new(Vec::new())
    }
}

/// Frames of idle typing before a field is checked (×100 ms).
const DEBOUNCE: usize = 4;

impl Connect {
    pub fn new(found: Vec<Found>) -> Self {
        Connect {
            stage: Stage::Pick,
            found,
            cursor: 0,
            paste: String::new(),
            demo: false,
            engine: Engine::Postgres,
            version: None,
            host: String::new(),
            port: String::new(),
            database: String::new(),
            sid: false,
            user: String::new(),
            pass_mode: PassMode::Type,
            password: String::new(),
            pass_env: String::new(),
            pass_file: String::new(),
            pass_saved: String::new(),
            tls_mode: TlsMode::Auto,
            ca_file: String::new(),
            ca_saved: String::new(),
            field: Field::Host,
            defaulted: Vec::new(),
            db_options: Vec::new(),
            completions: Vec::new(),
            live: Live::default(),
            generation: 0,
            edited_at: 0,
            want_verify: false,
            frame: 0,
            error: None,
            checks: Vec::new(),
            checks_at: 0,
            endpoint: String::new(),
            home: None,
            env_set: false,
        }
    }

    /// The pick list, in display order.
    pub fn choices(&self) -> Vec<Choice> {
        (0..self.found.len())
            .map(Choice::Found)
            .chain(Engine::ALL.into_iter().map(Choice::Engine))
            .chain([Choice::Demo])
            .collect()
    }

    /// Fields shown for the current engine.
    pub fn fields(&self) -> Vec<Field> {
        let mut f = vec![Field::Version, Field::Host, Field::Port, Field::Database];
        if self.engine == Engine::Oracle {
            f.push(Field::ConnectBy);
        }
        f.extend([Field::User, Field::Password]);
        if self.engine.uses_ca() {
            f.push(Field::Tls);
        }
        f
    }

    pub fn port_number(&self) -> Option<u16> {
        if self.port.trim().is_empty() {
            return Some(self.engine.port());
        }
        self.port.trim().parse().ok().filter(|p| *p > 0)
    }

    /// Start the form for an engine with every default filled.
    pub fn open_engine(&mut self, engine: Engine) {
        let prefill = Prefill {
            engine: Some(engine),
            ..Prefill::default()
        };
        self.open(&prefill, "");
    }

    /// Start the form from a prefill (import, recent, paste).
    pub fn open(&mut self, p: &Prefill, version: &str) {
        let engine = p.engine.unwrap_or(self.engine);
        let changed_engine = engine != self.engine;
        self.engine = engine;
        self.stage = Stage::Form;
        self.version = source::versions_of(engine)
            .iter()
            .position(|v| v.label == version);
        self.defaulted.clear();
        if !p.host.is_empty() || changed_engine || self.host.is_empty() {
            self.host = if p.host.is_empty() {
                self.defaulted.push(Field::Host);
                "localhost".into()
            } else {
                p.host.clone()
            };
        }
        self.port = p.port.map_or_else(String::new, |n| n.to_string());
        self.database = if p.database.is_empty() {
            self.defaulted.push(Field::Database);
            engine.default_database().to_owned()
        } else {
            p.database.clone()
        };
        self.sid = p.sid;
        self.user = if p.user.is_empty() {
            self.defaulted.push(Field::User);
            engine.suggested_user().to_owned()
        } else {
            p.user.clone()
        };
        self.pass_env = engine.password_env().to_owned();
        match (&p.password, &p.password_ref) {
            (Some(pw), _) => {
                self.pass_mode = PassMode::Type;
                self.password = pw.clone();
            }
            (None, Some(r)) if r.starts_with("env:") => {
                self.pass_mode = PassMode::Env;
                self.pass_env = r[4..].to_owned();
            }
            (None, Some(r)) if r.starts_with("file:") => {
                self.pass_mode = PassMode::File;
                self.pass_file = r[5..].to_owned();
            }
            (None, Some(r)) if oo_stack::stack::secret_name(r) => {
                self.pass_mode = PassMode::Saved;
                self.pass_saved = r.clone();
            }
            _ => self.pass_mode = PassMode::Type,
        }
        if !p.ca_ref.is_empty() && self.engine.uses_ca() {
            self.tls_mode = TlsMode::Saved;
            self.ca_saved = p.ca_ref.clone();
        } else if !p.ca_file.is_empty() {
            self.tls_mode = TlsMode::File;
            self.ca_file = p.ca_file.clone();
        } else {
            self.tls_mode = TlsMode::Auto;
        }
        self.db_options = self.database_candidates();
        self.live = Live::default();
        self.generation += 1;
        self.edited_at = 0;
        self.error = None;
        // Focus the first thing still to do: the host when only the
        // default is there, else the password.
        self.field = if self.defaulted.contains(&Field::Host) {
            Field::Host
        } else if self.pass_mode == PassMode::Type && self.password.is_empty() {
            Field::Password
        } else {
            Field::Host
        };
    }

    /// Known database / service names for this engine and host.
    fn database_candidates(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |s: &str| {
            if !s.is_empty() && !out.iter().any(|o| o == s) {
                out.push(s.to_owned());
            }
        };
        add(&self.database);
        if let Some(v) = &self.live.verified {
            for d in &v.databases {
                add(d);
            }
        }
        for f in &self.found {
            if f.prefill.engine == Some(self.engine)
                && f.prefill.host.eq_ignore_ascii_case(&self.host)
            {
                add(&f.prefill.database);
            }
        }
        for h in self.engine.database_hints() {
            add(h);
        }
        out
    }

    fn edited(&mut self) {
        self.generation += 1;
        self.edited_at = self.frame;
        self.error = None;
        self.completions.clear();
    }

    /// The PEM to trust for sign-in, if decided.
    pub fn ca_pem(&self) -> Option<String> {
        match self.tls_mode {
            TlsMode::Trust => self.anchor().map(CertInfo::pem),
            TlsMode::File => std::fs::read_to_string(self.ca_path()?)
                .ok()
                .filter(|p| p.contains("BEGIN CERTIFICATE")),
            TlsMode::Auto | TlsMode::Saved => None,
        }
    }

    /// CA file path with `~/` expanded.
    pub fn ca_path(&self) -> Option<PathBuf> {
        expand(self.ca_file.trim(), self.home.as_deref())
    }

    fn tls_report(&self) -> Option<&TlsReport> {
        let port = self.port_number()?;
        match &self.live.tls {
            Some(((h, p), Ok(r))) if *h == self.host && *p == port => Some(r),
            _ => None,
        }
    }

    fn tls_error(&self) -> Option<&'static str> {
        let port = self.port_number()?;
        match &self.live.tls {
            Some(((h, p), Err(e))) if *h == self.host && *p == port => Some(e),
            _ => None,
        }
    }

    /// The presented certificate that could be trusted.
    pub fn anchor(&self) -> Option<&CertInfo> {
        self.tls_report().and_then(TlsReport::anchor)
    }

    fn resolved(&self) -> Option<&Result<Vec<IpAddr>, &'static str>> {
        match &self.live.resolved {
            Some((h, r)) if *h == self.host => Some(r),
            _ => None,
        }
    }

    /// Every address the host resolves to is loopback or private.
    pub fn private_network(&self) -> bool {
        matches!(self.resolved(), Some(Ok(ips)) if ips.iter().all(|ip| source::private_ip(*ip)))
    }

    fn port_state(&self) -> Option<&Result<Duration, &'static str>> {
        let port = self.port_number()?;
        match &self.live.port {
            Some(((h, p), r)) if *h == self.host && *p == port => Some(r),
            _ => None,
        }
    }

    /// The current sign-in answer, if it is for this form state.
    pub fn verified(&self) -> Option<&Verified> {
        self.live
            .verified
            .as_ref()
            .filter(|v| v.generation == self.generation)
    }

    /// Version from the server: sign-in, or MySQL's greeting.
    pub fn detected_version(&self) -> Option<String> {
        self.verified()
            .and_then(|v| v.version.clone())
            .or_else(|| self.tls_report().and_then(|r| r.server_version.clone()))
    }

    /// Password reference for the stack (`env:` / `file:`), if one is used.
    pub fn password_ref(&self) -> Option<String> {
        match self.pass_mode {
            PassMode::Type => None,
            PassMode::Env => Some(format!("env:{}", self.pass_env.trim())),
            PassMode::File => expand(self.pass_file.trim(), self.home.as_deref())
                .map(|p| format!("file:{}", p.display())),
            PassMode::Saved => Some(self.pass_saved.clone()),
        }
    }

    /// Hand the typed password to the caller (to store it); clears it here.
    pub fn take_password(&mut self) -> Option<String> {
        (self.pass_mode == PassMode::Type && !self.password.is_empty())
            .then(|| std::mem::take(&mut self.password))
    }

    fn secret(&self) -> Secret {
        match self.password_ref() {
            Some(r) => Secret::Ref(r),
            None => Secret::Typed(self.password.clone()),
        }
    }

    /// First problem that blocks sign-in, as (field, error code).
    fn invalid(&self) -> Option<(Field, &'static str)> {
        if self.host.trim().is_empty() {
            return Some((Field::Host, "HOST_EMPTY"));
        }
        if matches!(self.resolved(), Some(Err(_))) {
            return Some((Field::Host, "HOST_UNRESOLVED"));
        }
        if self.port_number().is_none() {
            return Some((Field::Port, "DSN_PORT"));
        }
        if self.database.trim().is_empty() {
            return Some((Field::Database, "DATABASE_EMPTY"));
        }
        if self.user.trim().is_empty() {
            return Some((Field::User, "USER_EMPTY"));
        }
        match self.pass_mode {
            PassMode::Type if self.password.is_empty() => {
                return Some((Field::Password, "PASSWORD_EMPTY"));
            }
            PassMode::Env if !self.env_set => return Some((Field::Password, "SECRET_ENV")),
            PassMode::File
                if !self
                    .password_ref()
                    .is_some_and(|r| oo_stack::stack::secret_reference(&r)) =>
            {
                return Some((Field::Password, "SECRET_FILE"));
            }
            _ => (),
        }
        if self.engine.uses_ca() {
            match self.tls_mode {
                TlsMode::Auto => return Some((Field::Tls, "TLS_CHOOSE")),
                TlsMode::Trust if self.anchor().is_none() => {
                    return Some((Field::Tls, "TLS_CHOOSE"));
                }
                TlsMode::File if self.ca_pem().is_none() => {
                    return Some((Field::Tls, "CA_FILE"));
                }
                TlsMode::Trust | TlsMode::File | TlsMode::Saved => (),
            }
        }
        None
    }

    /// Ready to leave: everything valid and sign-in answered for this state.
    pub fn ready(&self) -> bool {
        self.invalid().is_none() && self.verified().is_some_and(|v| v.auth.is_ok())
    }

    /// Live checks to start now (debounced). The runner skips keys already
    /// running or answered.
    pub fn wants(&self) -> Vec<Job> {
        let mut jobs = Vec::new();
        if self.stage != Stage::Form {
            return jobs;
        }
        let settled = self.frame.saturating_sub(self.edited_at) >= DEBOUNCE
            || !matches!(self.field, Field::Host | Field::Port);
        let host = self.host.trim();
        let Some(port) = self.port_number() else {
            return jobs;
        };
        if !settled || host.is_empty() {
            return jobs;
        }
        if self.resolved().is_none() {
            jobs.push(Job::Resolve(host.to_owned()));
            return jobs;
        }
        if !matches!(self.resolved(), Some(Ok(_))) {
            return jobs;
        }
        if self.port_state().is_none() {
            jobs.push(Job::Port(host.to_owned(), port));
            return jobs;
        }
        if matches!(self.port_state(), Some(Ok(_)))
            && self.engine != Engine::Oracle
            && self.tls_report().is_none()
            && self.tls_error().is_none()
        {
            jobs.push(Job::Tls(self.engine, host.to_owned(), port));
        }
        jobs
    }

    /// The sign-in request to run now, if ⏎ asked for one.
    pub fn take_verify(&mut self) -> Option<Job> {
        if !std::mem::take(&mut self.want_verify) {
            return None;
        }
        self.live.verifying = Some(self.generation);
        Some(Job::Verify(Request {
            engine: self.engine,
            host: self.host.trim().to_owned(),
            port: self.port_number()?,
            database: self.database.trim().to_owned(),
            user: self.user.trim().to_owned(),
            secret: self.secret(),
            ca_pem: self.ca_pem(),
            ca_ref: (self.tls_mode == TlsMode::Saved).then(|| self.ca_saved.clone()),
            generation: self.generation,
        }))
    }

    /// Fold a finished job into the state.
    pub fn apply(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Resolved(h, r) => self.live.resolved = Some((h, r)),
            Outcome::Port(h, p, r) => self.live.port = Some(((h, p), r)),
            Outcome::Tls(h, p, r) => {
                // A certificate answer with a ready decision: keep it. The
                // user's CA file always wins.
                if let Ok(report) = &r
                    && let Some(v) = &report.server_version
                    && self.version.is_none()
                {
                    self.version = source::match_version(self.engine, v);
                }
                self.live.tls = Some(((h, p), r));
            }
            Outcome::Verified(v) => {
                if self.live.verifying == Some(v.generation) {
                    self.live.verifying = None;
                }
                if v.generation == self.generation {
                    if let Some(raw) = &v.version
                        && let Some(i) = source::match_version(self.engine, raw)
                    {
                        self.version = Some(i);
                    }
                    self.live.verified = Some(v);
                    self.db_options = self.database_candidates();
                }
            }
        }
    }

    /// The focused field held only a default: it no longer does.
    fn take_default(&mut self) -> bool {
        let field = self.field;
        let before = self.defaulted.len();
        self.defaulted.retain(|f| *f != field);
        before != self.defaulted.len()
    }

    fn focus(&mut self, delta: isize) {
        let fields = self.fields();
        let at = fields.iter().position(|f| *f == self.field).unwrap_or(0) as isize;
        let next = (at + delta).rem_euclid(fields.len() as isize) as usize;
        self.field = fields[next];
        self.completions.clear();
    }

    fn text_mut(&mut self) -> Option<&mut String> {
        Some(match self.field {
            Field::Host => &mut self.host,
            Field::Port => &mut self.port,
            Field::Database => &mut self.database,
            Field::User => &mut self.user,
            Field::Password => match self.pass_mode {
                PassMode::Type => &mut self.password,
                PassMode::Env => &mut self.pass_env,
                PassMode::File => &mut self.pass_file,
                PassMode::Saved => return None,
            },
            Field::Tls if self.tls_mode == TlsMode::File => &mut self.ca_file,
            _ => return None,
        })
    }

    fn path_field(&self) -> bool {
        (self.field == Field::Password && self.pass_mode == PassMode::File)
            || (self.field == Field::Tls && self.tls_mode == TlsMode::File)
    }

    fn cycle(&mut self, delta: isize) {
        let step = |i: usize, n: usize| (i as isize + delta).rem_euclid(n as isize) as usize;
        match self.field {
            Field::Version => {
                let n = source::versions_of(self.engine).len();
                // auto-detect, then each version.
                let at = self.version.map_or(0, |v| v + 1);
                let next = step(at, n + 1);
                self.version = next.checked_sub(1);
            }
            Field::Database if !self.db_options.is_empty() => {
                let at = self
                    .db_options
                    .iter()
                    .position(|d| *d == self.database)
                    .unwrap_or(0);
                self.database = self.db_options[step(at, self.db_options.len())].clone();
                self.take_default();
                self.edited();
            }
            Field::ConnectBy => {
                self.sid = !self.sid;
                self.edited();
            }
            Field::Password => {
                let mut modes = vec![PassMode::Type, PassMode::Env, PassMode::File];
                if !self.pass_saved.is_empty() {
                    modes.insert(0, PassMode::Saved);
                }
                let at = modes.iter().position(|m| *m == self.pass_mode).unwrap_or(0);
                self.pass_mode = modes[step(at, modes.len())];
                self.edited();
            }
            Field::Tls => {
                let mut modes: Vec<TlsMode> = if self.anchor().is_some() && self.private_network() {
                    vec![TlsMode::Trust, TlsMode::File]
                } else {
                    vec![TlsMode::File]
                };
                if !self.ca_saved.is_empty() {
                    modes.insert(0, TlsMode::Saved);
                }
                let at = modes.iter().position(|m| *m == self.tls_mode);
                self.tls_mode = match at {
                    Some(i) => modes[step(i, modes.len())],
                    None => modes[if delta > 0 { 0 } else { modes.len() - 1 }],
                };
                self.edited();
            }
            _ => (),
        }
    }

    fn complete(&mut self) {
        let home = self.home.clone();
        let (text, options) = {
            let Some(t) = self.text_mut() else { return };
            source::complete_path(t, home.as_deref())
        };
        if let Some(t) = self.text_mut() {
            *t = text;
        }
        self.edited();
        self.completions = options;
    }
}

fn expand(path: &str, home: Option<&std::path::Path>) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }
    match (path.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => Some(h.join(rest)),
        (Some(_), None) => None,
        _ => {
            let p = PathBuf::from(path);
            p.is_absolute().then_some(p)
        }
    }
}

/// Feed pasted text (bracketed paste) into the screen.
pub fn paste(state: &mut Connect, text: &str) {
    let text: String = text.chars().filter(|c| *c != '\r').take(8192).collect();
    match state.stage {
        Stage::Pick => state.paste.push_str(&text),
        Stage::Form => {
            // A whole URL pasted into the form refills it.
            if let Ok(p) = source::parse_any(&text)
                && (p.engine.is_some() || !p.host.is_empty())
                && text.contains(['/', '@', '=', ':'])
                && state.field != Field::Password
            {
                state.open(&p, "");
                return;
            }
            let one_line: String = text.lines().next().unwrap_or("").to_owned();
            if let Some(t) = state.text_mut() {
                t.push_str(&one_line);
            }
            state.edited();
        }
        Stage::Report => (),
    }
}

pub fn key(state: &mut Connect, key: KeyEvent) -> Action {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    match state.stage {
        Stage::Pick => pick_key(state, key),
        Stage::Form => form_key(state, key),
        Stage::Report => match key.code {
            KeyCode::Enter => Action::Next,
            KeyCode::Esc => Action::Quit,
            _ => Action::None,
        },
    }
}

fn pick_key(state: &mut Connect, key: KeyEvent) -> Action {
    let choices = state.choices();
    match key.code {
        KeyCode::Esc if !state.paste.is_empty() => state.paste.clear(),
        KeyCode::Esc => return Action::Quit,
        KeyCode::Up => state.cursor = state.cursor.saturating_sub(1),
        KeyCode::Down => state.cursor = (state.cursor + 1).min(choices.len() - 1),
        KeyCode::Backspace => {
            state.paste.pop();
        }
        KeyCode::Enter if !state.paste.trim().is_empty() => match source::parse_any(&state.paste) {
            Ok(p) if p.engine.is_some() || !p.host.is_empty() => {
                let p = Prefill {
                    engine: p.engine.or(Some(Engine::Postgres)),
                    ..p
                };
                state.paste.clear();
                state.open(&p, "");
            }
            _ => state.error = Some(("PASTE_UNKNOWN", "")),
        },
        KeyCode::Enter => match choices.get(state.cursor) {
            Some(Choice::Found(i)) => {
                let f = state.found[*i].clone();
                state.open(&f.prefill, &f.version);
            }
            Some(Choice::Engine(e)) => state.open_engine(*e),
            Some(Choice::Demo) => {
                state.demo = true;
                return Action::Next;
            }
            None => (),
        },
        KeyCode::Char(c)
            if !c.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && state.paste.len() < 8192 =>
        {
            state.paste.push(c);
            state.error = None;
        }
        _ => (),
    }
    Action::None
}

fn form_key(state: &mut Connect, key: KeyEvent) -> Action {
    let plain = !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    match key.code {
        KeyCode::Esc => {
            if !state.completions.is_empty() {
                state.completions.clear();
            } else {
                state.stage = Stage::Pick;
                state.error = None;
            }
        }
        KeyCode::Up => state.focus(-1),
        KeyCode::Down => state.focus(1),
        KeyCode::BackTab => state.focus(-1),
        KeyCode::Tab if state.path_field() => state.complete(),
        KeyCode::Tab => state.focus(1),
        KeyCode::Left => state.cycle(-1),
        KeyCode::Right => state.cycle(1),
        KeyCode::Backspace => {
            let replace = state.take_default();
            if let Some(t) = state.text_mut() {
                if replace {
                    t.clear();
                } else {
                    t.pop();
                }
                state.edited();
            }
        }
        // `t` on the TLS row trusts the presented CA (private networks).
        KeyCode::Char('t')
            if state.field == Field::Tls
                && state.tls_mode != TlsMode::File
                && state.anchor().is_some()
                && state.private_network() =>
        {
            state.tls_mode = TlsMode::Trust;
            state.edited();
        }
        KeyCode::Char('c') if state.field == Field::Tls && state.tls_mode != TlsMode::File => {
            state.tls_mode = TlsMode::File;
            state.edited();
        }
        KeyCode::Char(c) if !c.is_control() && plain => {
            if state.field == Field::Port && !c.is_ascii_digit() {
                return Action::None;
            }
            let replace = state.take_default();
            if let Some(t) = state.text_mut()
                && t.len() + c.len_utf8() <= 4096
            {
                if replace {
                    t.clear();
                }
                t.push(c);
                state.edited();
            }
        }
        KeyCode::Enter => {
            if !state.completions.is_empty() {
                state.completions.clear();
                return Action::None;
            }
            if let Some((field, code)) = state.invalid() {
                state.field = field;
                state.error = Some((code, ""));
                return Action::None;
            }
            if state.ready() {
                return Action::Next;
            }
            if state.live.verifying != Some(state.generation) {
                state.want_verify = true;
            }
        }
        _ => (),
    }
    Action::None
}

// ----------------------------------------------------------------- render

pub const PICK_HINTS: [Hint; 4] = [
    ("↑↓", "choose", "选择"),
    ("⏎", "next", "下一步"),
    ("›", "type or paste anything", "输入或粘贴任意连接信息"),
    ("esc", "quit", "退出"),
];

pub const FORM_HINTS: [Hint; 5] = [
    ("↑↓", "field", "字段"),
    ("←→", "option", "选项"),
    ("tab", "complete", "补全"),
    ("⏎", "connect", "连接"),
    ("esc", "back", "返回"),
];

pub const REPORT_HINTS: [Hint; 2] = [("⏎", "continue", "继续"), ("esc", "quit", "退出")];

/// `▐PG▌` badge in the engine's tone.
fn badge(theme: &Theme, engine: Engine) -> Span<'static> {
    let tone = match engine {
        Engine::Postgres => Tone::Info,
        Engine::Mysql => Tone::Warn,
        Engine::Oracle => Tone::Bad,
    };
    Span::styled(
        format!("▐{}▌", engine.badge()),
        theme
            .style(tone)
            .add_modifier(ratatui::style::Modifier::REVERSED),
    )
}

fn level_tone(level: &str) -> Tone {
    match level {
        "verified" => Tone::Ok,
        "experimental" | "partly verified" => Tone::Warn,
        _ => Tone::Faint,
    }
}

fn level_word<'a>(level: &'a str, theme: &Theme) -> &'a str {
    match level {
        "verified" => theme.t("verified", "已验证"),
        "experimental" => theme.t("experimental", "实验"),
        "partly verified" => theme.t("partly verified", "部分验证"),
        "designed" => theme.t("designed", "设计中"),
        other => other,
    }
}

/// One line about an engine's versions and change capture.
fn versions_line(engine: Engine, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, v) in source::versions_of(engine).iter().enumerate() {
        if i > 0 {
            spans.push(theme.span(" ", Tone::Faint));
        }
        spans.push(theme.span(v.label.clone(), Tone::Ink));
        spans.push(theme.span("●", level_tone(&v.full)));
    }
    let cdc_all = source::versions_of(engine)
        .iter()
        .all(|v| v.cdc == "verified");
    spans.push(theme.span(
        if cdc_all {
            theme.t("   CDC ● all", "   CDC ● 全部")
        } else {
            theme.t("   CDC varies by version", "   CDC 因版本而异")
        },
        Tone::Dim,
    ));
    spans
}

pub fn render(state: &Connect, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let hints: &[Hint] = match state.stage {
        Stage::Pick => &PICK_HINTS,
        Stage::Form => &FORM_HINTS,
        Stage::Report => &REPORT_HINTS,
    };
    let inner = theme::frame(buf, area, theme, 0, hints);
    if inner.height < 8 {
        return;
    }
    match state.stage {
        Stage::Pick => render_pick(state, inner, buf, theme),
        Stage::Form => render_form(state, inner, buf, theme),
        Stage::Report => render_report(state, inner, buf, theme),
    }
}

fn error_text(code: &str, theme: &Theme) -> &'static str {
    match code {
        "PASTE_UNKNOWN" => theme.t(
            "not understood: try a URL, JDBC URL, KEY=VALUE lines or host:port",
            "没看懂：试试 URL、JDBC URL、KEY=VALUE 行或 主机:端口",
        ),
        "HOST_EMPTY" => theme.t("host is empty", "主机为空"),
        "HOST_UNRESOLVED" => theme.t("host does not resolve", "主机名解析不到"),
        "DSN_PORT" => theme.t("port must be 1–65535", "端口须在 1–65535"),
        "DATABASE_EMPTY" => theme.t("choose a database", "选一个数据库"),
        "USER_EMPTY" => theme.t("user is empty", "用户为空"),
        "PASSWORD_EMPTY" => theme.t(
            "type the password, or ←→ to read it from a variable or file",
            "输入密码，或按 ←→ 改为从环境变量或文件读取",
        ),
        "SECRET_ENV" => theme.t("that variable is not set", "这个环境变量没有设置"),
        "SECRET_FILE" => theme.t(
            "give an absolute path to the password file",
            "给出密码文件的绝对路径",
        ),
        "TLS_CHOOSE" => theme.t(
            "choose how to trust the server: t this CA · c CA file",
            "选择如何信任服务器：t 信任此 CA · c 选 CA 文件",
        ),
        "CA_FILE" => theme.t(
            "that file is not a PEM certificate (tab completes paths)",
            "这不是 PEM 证书文件（tab 补全路径）",
        ),
        _ => theme.t("check this field", "请检查这一项"),
    }
}

fn render_pick(state: &Connect, inner: Rect, buf: &mut Buffer, theme: &Theme) {
    heading(
        buf,
        inner.x,
        inner.y,
        inner.width,
        theme,
        theme.t("connect", "连接"),
        theme.t(
            "read-only · nothing is written to your database",
            "只读 · 不会写入你的数据库",
        ),
    );
    let mut y = inner.y + 2;
    // Paste line.
    if state.paste.is_empty() {
        put(
            buf,
            inner.x,
            y,
            inner.width,
            vec![
                theme.span("› ", Tone::Accent),
                theme.span(
                    fit(
                        theme.t(
                            "paste or type a URL, JDBC URL, DATABASE_URL=… or host:port",
                            "粘贴或输入 URL、JDBC URL、DATABASE_URL=… 或 主机:端口",
                        ),
                        usize::from(inner.width).saturating_sub(3),
                    ),
                    Tone::Faint,
                ),
            ],
        );
    } else {
        input(
            buf,
            inner.x,
            y,
            inner.width,
            theme,
            &source::mask(&state.paste),
            true,
        );
    }
    y += 1;
    if !state.paste.trim().is_empty() {
        let line = match (source::parse_any(&state.paste), state.error) {
            (_, Some((code, _))) => {
                vec![theme.span(format!("✗ {}", error_text(code, theme)), Tone::Bad)]
            }
            (Ok(p), _) => vec![
                theme.span("✓ ", Tone::Ok),
                theme.span(format!("{}  ", p.kind.text(theme.zh)), Tone::Dim),
                theme.span(
                    fit(
                        &p.summary(theme.zh),
                        usize::from(inner.width).saturating_sub(24),
                    ),
                    Tone::Ink,
                ),
                theme.span(theme.t("  ⏎ use", "  ⏎ 使用"), Tone::Accent),
            ],
            (Err(_), _) => vec![theme.span(theme.t("… keep typing", "… 继续输入"), Tone::Faint)],
        };
        put(buf, inner.x + 2, y, inner.width.saturating_sub(2), line);
    }
    y += 1;
    let choices = state.choices();
    let wide = inner.width >= 100;
    let mut section = "";
    for (i, choice) in choices.iter().enumerate() {
        let name = match choice {
            Choice::Found(_) => theme.t("found on this machine", "本机已有"),
            Choice::Engine(_) => theme.t("engines", "数据库引擎"),
            Choice::Demo => "",
        };
        if name != section && !name.is_empty() {
            if y + 2 >= inner.bottom() {
                break;
            }
            y += 1;
            put(
                buf,
                inner.x,
                y,
                inner.width,
                vec![theme.span(name, Tone::Dim)],
            );
            y += 1;
            section = name;
        }
        if y + 1 >= inner.bottom() {
            break;
        }
        let on = i == state.cursor && state.paste.is_empty();
        let mut spans = vec![theme.span(if on { "▸ " } else { "  " }, Tone::Accent)];
        match choice {
            Choice::Found(k) => {
                let f = &state.found[*k];
                let e = f.prefill.engine.unwrap_or(Engine::Postgres);
                spans.push(badge(theme, e));
                let origin = match (f.origin.as_str(), f.name.is_empty()) {
                    ("recent", _) => format!(" {} {}", theme.t("recent", "最近"), f.name),
                    ("tnsnames.ora", false) => format!(" tns {}", f.name),
                    (o, true) => format!(" {o}"),
                    (o, false) => format!(" {o} {}", f.name),
                };
                spans.push(theme.span(theme::pad(&origin, 20), Tone::Dim));
                let summary = f.prefill.compact();
                let tail = if f.version.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", f.version)
                };
                let text = fit(
                    &format!("{summary}{tail}"),
                    usize::from(inner.width).saturating_sub(24),
                );
                spans.push(if on {
                    Span::styled(text, theme.selected())
                } else {
                    theme.span(text, Tone::Ink)
                });
            }
            Choice::Engine(e) => {
                spans.push(badge(theme, *e));
                let name = format!(" {}", theme::pad(e.name(), 12));
                spans.push(if on {
                    Span::styled(name, theme.selected())
                } else {
                    theme.span(name, Tone::Ink)
                });
                spans.push(theme.span(
                    format!(" {}", theme::pad(&source::version_range(*e), 10)),
                    Tone::Dim,
                ));
                let level = source::engine_level(*e);
                spans.push(theme.span("● ", level_tone(level)));
                spans.push(theme.span(level_word(level, theme), Tone::Dim));
                if wide {
                    spans.push(theme.span(format!("   :{}", e.port()), Tone::Faint));
                }
            }
            Choice::Demo => {
                if y + 1 < inner.bottom() {
                    y += 1;
                }
                spans.push(theme.span("◇ ", Tone::Accent));
                let text = theme
                    .t("no database? try the demo", "没有数据库？用示例库")
                    .to_owned();
                spans.push(if on {
                    Span::styled(text, theme.selected())
                } else {
                    theme.span(text, Tone::Ink)
                });
            }
        }
        put(buf, inner.x, y, inner.width, spans);
        y += 1;
    }
    // What the hovered engine supports, at the bottom.
    if let Some(Choice::Engine(e)) = choices.get(state.cursor)
        && state.paste.is_empty()
    {
        let foot = inner.bottom() - 1;
        let mut spans = vec![theme.span(theme::pad(theme.t("versions", "版本"), 10), Tone::Dim)];
        spans.extend(versions_line(*e, theme));
        put(buf, inner.x, foot, inner.width, spans);
    } else if let Some(Choice::Found(_)) = choices.get(state.cursor) {
        put(
            buf,
            inner.x,
            inner.bottom() - 1,
            inner.width,
            vec![theme.span(
                theme.t(
                    "passwords from these files are moved to the secret store, never shown",
                    "这些文件里的密码会存进密钥库，不会显示",
                ),
                Tone::Faint,
            )],
        );
    }
}

/// Status column text for a field: (glyph, text, tone).
fn status(state: &Connect, field: Field, theme: &Theme) -> Option<(String, String, Tone)> {
    let spin = match theme.frame(state.frame) {
        usize::MAX => "…".to_owned(),
        f => fx::spinner(f).to_string(),
    };
    let wait = |text: &str| Some((spin.clone(), text.to_owned(), Tone::Accent));
    match field {
        Field::Version => {
            let versions = source::versions_of(state.engine);
            let detected = state.detected_version();
            match (state.version.and_then(|i| versions.get(i)), &detected) {
                (Some(v), Some(raw)) => Some((
                    "✓".into(),
                    format!(
                        "{} {raw} · {}",
                        theme.t("detected", "检测到"),
                        level_word(&v.full, theme)
                    ),
                    Tone::Ok,
                )),
                (None, Some(raw)) => Some((
                    "!".into(),
                    if theme.zh {
                        format!("检测到 {raw}，不在支持矩阵内")
                    } else {
                        format!("detected {raw}: not in the support matrix")
                    },
                    Tone::Warn,
                )),
                (Some(v), None) => Some((
                    "●".into(),
                    format!(
                        "{} · CDC {}",
                        level_word(&v.full, theme),
                        level_word(&v.cdc, theme)
                    ),
                    level_tone(&v.full),
                )),
                (None, None) => Some((
                    "·".into(),
                    format!(
                        "{} · {}",
                        source::version_range(state.engine),
                        theme.t("detected on connect", "连接后自动检测")
                    ),
                    Tone::Faint,
                )),
            }
        }
        Field::Host => match state.resolved() {
            Some(Ok(ips)) => Some((
                "✓".into(),
                format!(
                    "{}{}",
                    ips[0],
                    if ips.len() > 1 {
                        format!(" +{}", ips.len() - 1)
                    } else {
                        String::new()
                    }
                ),
                Tone::Ok,
            )),
            Some(Err(_)) => Some((
                "✗".into(),
                error_text("HOST_UNRESOLVED", theme).into(),
                Tone::Bad,
            )),
            None if !state.host.trim().is_empty() => wait(theme.t("resolving…", "解析中…")),
            None => None,
        },
        Field::Port => match state.port_state() {
            Some(Ok(d)) => Some((
                "✓".into(),
                format!("{} · {} ms", theme.t("open", "端口可达"), d.as_millis()),
                Tone::Ok,
            )),
            Some(Err(e)) => Some((
                "✗".into(),
                if *e == "PORT_TIMEOUT" {
                    theme.t("no answer (firewall?)", "无响应（防火墙？）")
                } else {
                    theme.t("closed", "端口未开放")
                }
                .into(),
                Tone::Bad,
            )),
            None if matches!(state.resolved(), Some(Ok(_))) => {
                wait(theme.t("checking…", "检查中…"))
            }
            None if state.port.is_empty() => {
                Some(("·".into(), theme.t("default", "默认").into(), Tone::Faint))
            }
            None => None,
        },
        Field::Database => {
            let n = state.db_options.len();
            (n > 1).then(|| {
                (
                    "‹›".into(),
                    if theme.zh {
                        format!("←→ {n} 个可选")
                    } else {
                        format!("←→ {n} known")
                    },
                    Tone::Faint,
                )
            })
        }
        Field::ConnectBy => Some((
            "·".into(),
            if state.sid {
                theme.t("connected as a service name", "按服务名连接")
            } else {
                theme.t("PDB service, e.g. ORCLPDB1", "PDB 服务名，如 ORCLPDB1")
            }
            .into(),
            Tone::Faint,
        )),
        Field::User => (state.user == state.engine.suggested_user()).then(|| {
            (
                "·".into(),
                theme
                    .t("suggested read-only account", "建议的只读账号")
                    .into(),
                Tone::Faint,
            )
        }),
        Field::Password => match state.pass_mode {
            PassMode::Type => Some((
                "·".into(),
                theme.t("kept in the secret store", "存进密钥库").into(),
                Tone::Faint,
            )),
            PassMode::Env if state.env_set => Some((
                "✓".into(),
                theme
                    .t("set · referenced, not copied", "已设置 · 只引用不复制")
                    .into(),
                Tone::Ok,
            )),
            PassMode::Env => Some((
                "✗".into(),
                error_text("SECRET_ENV", theme).into(),
                Tone::Bad,
            )),
            PassMode::File => Some((
                "·".into(),
                theme.t("referenced, not copied", "只引用不复制").into(),
                Tone::Faint,
            )),
            PassMode::Saved => Some((
                "✓".into(),
                theme.t("saved earlier", "之前已保存").into(),
                Tone::Ok,
            )),
        },
        Field::Tls => {
            if let Some(e) = state.tls_error() {
                return Some((
                    "✗".into(),
                    match e {
                        "TLS_NOT_OFFERED" => theme.t(
                            "server offers no TLS; oo requires it",
                            "服务器未提供 TLS；oo 要求 TLS",
                        ),
                        "TLS_13_ONLY" => theme.t(
                            "TLS 1.3 only: choose its CA file",
                            "仅支持 TLS 1.3：请选 CA 文件",
                        ),
                        _ => theme.t("could not read the certificate", "读不到证书"),
                    }
                    .into(),
                    Tone::Warn,
                ));
            }
            match (state.tls_mode, state.tls_report()) {
                (TlsMode::Saved, _) => Some((
                    "✓".into(),
                    theme.t("CA saved earlier", "之前保存的 CA").into(),
                    Tone::Ok,
                )),
                (TlsMode::File, _) => Some((
                    if state.ca_pem().is_some() {
                        "✓"
                    } else {
                        "·"
                    }
                    .into(),
                    theme
                        .t("verify with this CA file", "用这个 CA 文件校验")
                        .into(),
                    if state.ca_pem().is_some() {
                        Tone::Ok
                    } else {
                        Tone::Faint
                    },
                )),
                (TlsMode::Trust, Some(_)) => Some((
                    "✓".into(),
                    theme
                        .t("trusting the presented CA", "信任服务器出示的 CA")
                        .into(),
                    Tone::Ok,
                )),
                (_, Some(r)) => {
                    let leaf = &r.chain[0];
                    Some((
                        "!".into(),
                        format!(
                            "{} {} · {}",
                            theme.t("presents", "出示"),
                            leaf.subject,
                            theme.t("choose below", "请在下方选择")
                        ),
                        Tone::Warn,
                    ))
                }
                (_, None) if matches!(state.port_state(), Some(Ok(_))) => {
                    wait(theme.t("reading certificate…", "读取证书中…"))
                }
                _ => None,
            }
        }
    }
}

fn render_form(state: &Connect, inner: Rect, buf: &mut Buffer, theme: &Theme) {
    let e = state.engine;
    let mut title = vec![
        badge(theme, e),
        theme.span(format!(" {}", e.name()), Tone::Ink),
    ];
    title.push(theme.span(format!("  ·  {}", theme.t("read-only", "只读")), Tone::Dim));
    put(buf, inner.x, inner.y, inner.width, title);
    let label_w = 11usize;
    let value_w = if inner.width >= 100 { 40 } else { 28 };
    let status_x = inner.x + (2 + label_w + value_w + 1) as u16;
    let status_w = inner.right().saturating_sub(status_x);
    let mut y = inner.y + 2;
    let versions = source::versions_of(e);
    for field in state.fields() {
        if y >= inner.bottom() {
            break;
        }
        let on = field == state.field;
        let label = match field {
            Field::Version => theme.t("version", "版本"),
            Field::Host => theme.t("host", "主机"),
            Field::Port => theme.t("port", "端口"),
            Field::Database if e == Engine::Oracle && state.sid => "SID",
            Field::Database if e == Engine::Oracle => theme.t("service", "服务名"),
            Field::Database => theme.t("database", "数据库"),
            Field::ConnectBy => theme.t("connect by", "连接方式"),
            Field::User => theme.t("user", "用户"),
            Field::Password => theme.t("password", "密码"),
            Field::Tls => "TLS",
        };
        let options = |items: &[&str], at: usize| -> String {
            items
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    if i == at {
                        format!("‹{s}›")
                    } else {
                        (*s).to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let (value, tone, caret) = match field {
            Field::Version => (
                match state.version.and_then(|i| versions.get(i)) {
                    Some(v) => format!("‹ {} ›", v.label),
                    None => format!("‹ {} ›", theme.t("auto-detect", "自动检测")),
                },
                Tone::Ink,
                false,
            ),
            Field::Host => (state.host.clone(), Tone::Ink, true),
            Field::Port if state.port.is_empty() => (e.port().to_string(), Tone::Dim, true),
            Field::Port => (state.port.clone(), Tone::Ink, true),
            Field::Database => (state.database.clone(), Tone::Ink, true),
            Field::ConnectBy => (
                options(
                    &[theme.t("service name", "服务名"), "SID"],
                    usize::from(state.sid),
                ),
                Tone::Ink,
                false,
            ),
            Field::User => (state.user.clone(), Tone::Ink, true),
            Field::Password => match state.pass_mode {
                PassMode::Type => (
                    if state.password.is_empty() {
                        String::new()
                    } else {
                        MASK.to_owned()
                    },
                    Tone::Ink,
                    true,
                ),
                PassMode::Env => (format!("${}", state.pass_env), Tone::Info, true),
                PassMode::File => (format!("file {}", state.pass_file), Tone::Info, true),
                PassMode::Saved => (format!("{MASK} {}", state.pass_saved), Tone::Dim, false),
            },
            Field::Tls => match state.tls_mode {
                TlsMode::Auto => (format!("‹ {} ›", theme.t("auto", "自动")), Tone::Ink, false),
                TlsMode::Trust => (
                    format!("‹ {} ›", theme.t("trust server CA", "信任服务器 CA")),
                    Tone::Ink,
                    false,
                ),
                TlsMode::File => (format!("CA {}", state.ca_file), Tone::Info, true),
                TlsMode::Saved => (format!("CA {}", state.ca_saved), Tone::Dim, false),
            },
        };
        let mut spans = vec![
            theme.span(if on { "▸ " } else { "  " }, Tone::Accent),
            theme.span(
                theme::pad(label, label_w),
                if on { Tone::Accent } else { Tone::Dim },
            ),
        ];
        // Keep the tail of long values visible while typing.
        let room = value_w.saturating_sub(2);
        let mut shown = value.clone();
        while width(&shown) > room {
            shown.remove(0);
        }
        let shown_w = width(&shown);
        let tone = if state.defaulted.contains(&field) {
            Tone::Dim
        } else {
            tone
        };
        spans.push(theme.span(shown, tone));
        let mut used = shown_w;
        if on && caret {
            spans.push(theme.span("▏", Tone::Accent));
            used += 1;
        }
        // Password mode switch beside the value.
        if field == Field::Password && on {
            let mut modes = vec![theme.t("type", "输入"), "$ENV", theme.t("file", "文件")];
            if !state.pass_saved.is_empty() {
                modes.insert(0, theme.t("saved", "已存"));
            }
            let at =
                match state.pass_mode {
                    PassMode::Saved => 0,
                    PassMode::Type => 0,
                    PassMode::Env => 1,
                    PassMode::File => 2,
                } + usize::from(!state.pass_saved.is_empty() && state.pass_mode != PassMode::Saved);
            let text = format!(" {}", options(&modes, at));
            used += width(&text);
            spans.push(theme.span(text, Tone::Faint));
        }
        spans.push(theme.span(" ".repeat(value_w.saturating_sub(used)), Tone::Faint));
        put(buf, inner.x, y, (2 + label_w + value_w) as u16, spans);
        if let Some((glyph, text, tone)) = status(state, field, theme)
            && status_w > 4
        {
            put(
                buf,
                status_x,
                y,
                status_w,
                vec![
                    theme.span(format!("{glyph} "), tone),
                    theme.span(
                        fit(
                            &text,
                            usize::from(status_w).saturating_sub(width(&glyph) + 1),
                        ),
                        if tone == Tone::Faint {
                            Tone::Faint
                        } else {
                            Tone::Dim
                        },
                    ),
                ],
            );
        }
        y += 1;
    }
    // Detail area: completions, certificate, sign-in, errors.
    y += 1;
    let w = usize::from(inner.width);
    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    if !state.completions.is_empty() {
        lines.push(vec![
            theme.span(theme.t("tab  ", "tab  "), Tone::Accent),
            theme.span(
                fit(&state.completions.join("  "), w.saturating_sub(6)),
                Tone::Dim,
            ),
        ]);
    }
    if let Some(r) = state.tls_report()
        && e.uses_ca()
    {
        let leaf = &r.chain[0];
        let top = r.anchor().unwrap_or(leaf);
        lines.push(vec![
            theme.span(theme::pad(theme.t("certificate", "证书"), 12), Tone::Dim),
            theme.span(
                fit(
                    &format!(
                        "{} · {} {} · {} {}",
                        leaf.subject,
                        theme.t("issuer", "签发"),
                        if leaf.self_signed {
                            theme.t("itself", "自签").to_owned()
                        } else {
                            leaf.issuer.clone()
                        },
                        theme.t("until", "有效至"),
                        leaf.not_after
                    ),
                    w.saturating_sub(12),
                ),
                Tone::Ink,
            ),
        ]);
        lines.push(vec![
            theme.span(theme::pad("sha256", 12), Tone::Dim),
            theme.span(
                format!(
                    "{}…{}",
                    &top.sha256[..16.min(top.sha256.len())],
                    &top.sha256[top.sha256.len().saturating_sub(8)..]
                ),
                Tone::Faint,
            ),
        ]);
        if !leaf.covers(state.host.trim()) {
            lines.push(vec![theme.span(
                fit(
                    &if theme.zh {
                        format!(
                            "! 证书写的是 {}，不是 {}；校验会失败",
                            leaf.names.first().unwrap_or(&leaf.subject),
                            state.host.trim()
                        )
                    } else {
                        format!(
                            "! the certificate names {}, not {}; verification will fail",
                            leaf.names.first().unwrap_or(&leaf.subject),
                            state.host.trim()
                        )
                    },
                    w,
                ),
                Tone::Warn,
            )]);
        }
        if state.tls_mode == TlsMode::Auto {
            let choice = match (r.anchor().is_some(), state.private_network()) {
                _ if r.ca_as_leaf() => theme.t(
                    "the server uses a CA certificate as its own; TLS clients refuse it. Issue a server certificate",
                    "服务器把 CA 证书当自己的证书用，TLS 客户端会拒绝；请签发服务器证书",
                ),
                (true, true) => theme.t(
                    "t trust this CA (private network) · c choose a CA file",
                    "t 信任此 CA（内网）· c 选 CA 文件",
                ),
                (true, false) => theme.t(
                    "public network: compare sha256 with your DBA, then c choose its CA file",
                    "公网：先和 DBA 核对 sha256，再按 c 选 CA 文件",
                ),
                (false, _) => theme.t(
                    "the server sent no CA: c choose the CA file (tab completes)",
                    "服务器没发 CA：按 c 选 CA 文件（tab 补全）",
                ),
            };
            lines.push(vec![
                theme.span("→ ", Tone::Accent),
                theme.span(fit(choice, w.saturating_sub(2)), Tone::Ink),
            ]);
        }
    }
    if state.live.verifying == Some(state.generation) {
        lines.push(vec![
            theme.span(
                match theme.frame(state.frame) {
                    usize::MAX => "… ".to_owned(),
                    f => format!("{} ", fx::spinner(f)),
                },
                Tone::Accent,
            ),
            theme.span(
                theme.t("signing in (read-only)…", "登录中（只读）…"),
                Tone::Dim,
            ),
        ]);
    } else if let Some(v) = state.verified() {
        match &v.auth {
            Ok(note) => lines.push(vec![
                theme.span(
                    if v.checked { "✓ " } else { "· " },
                    if v.checked { Tone::Ok } else { Tone::Faint },
                ),
                theme.span(
                    fit(
                        &if v.checked {
                            if theme.zh {
                                format!("已登录为 {}", state.user.trim())
                            } else {
                                format!("signed in as {}", state.user.trim())
                            }
                        } else {
                            note.clone()
                        },
                        w.saturating_sub(2),
                    ),
                    Tone::Ink,
                ),
                theme.span(theme.t("   ⏎ continue", "   ⏎ 继续"), Tone::Accent),
            ]),
            Err(reason) => lines.push(vec![
                theme.span("✗ ", Tone::Bad),
                theme.span(fit(reason, w.saturating_sub(2)), Tone::Bad),
            ]),
        }
    }
    if let Some((code, _)) = state.error {
        lines.push(vec![
            theme.span("✗ ", Tone::Bad),
            theme.span(fit(error_text(code, theme), w.saturating_sub(2)), Tone::Bad),
        ]);
    }
    for line in lines {
        if y >= inner.bottom() {
            break;
        }
        put(buf, inner.x, y, inner.width, line);
        y += 1;
    }
}

fn render_report(state: &Connect, inner: Rect, buf: &mut Buffer, theme: &Theme) {
    heading(
        buf,
        inner.x,
        inner.y,
        inner.width,
        theme,
        theme.t("read-only checks", "只读检查"),
        theme.t(
            "advice only; oo never runs DBA statements",
            "只给建议；oo 从不执行 DBA 语句",
        ),
    );
    put(
        buf,
        inner.x,
        inner.y + 2,
        inner.width,
        vec![theme.span(fit(&state.endpoint, usize::from(inner.width)), Tone::Dim)],
    );
    let frame = theme.frame(state.frame);
    let shown = if frame == usize::MAX {
        usize::MAX
    } else {
        frame.saturating_sub(state.checks_at) + 1
    };
    checklist(
        buf,
        Rect::new(
            inner.x,
            inner.y + 4,
            inner.width,
            inner.height.saturating_sub(4),
        ),
        theme,
        &state.checks,
        state.frame,
        shown,
    );
}

/// The Report stage with the doctor's checks.
pub fn report(endpoint: &str, checks: Vec<Check>) -> Connect {
    let mut s = Connect::new(Vec::new());
    s.stage = Stage::Report;
    s.endpoint = endpoint.to_owned();
    s.checks = checks;
    s
}

/// `engine host:port / database as user` with no secret.
pub fn endpoint(state: &Connect) -> String {
    format!(
        "{} {}:{} / {} · {}",
        state.engine.name(),
        state.host.trim(),
        state.port_number().unwrap_or(0),
        state.database.trim(),
        state.user.trim()
    )
}

/// The version the user picked or the server reported.
pub fn version_label(state: &Connect) -> String {
    state
        .version
        .and_then(|i| {
            source::versions_of(state.engine)
                .get(i)
                .map(|v: &Version| v.label.clone())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::{Status, snapshot};
    use super::*;
    use crate::tui::netprobe::parse_cert;

    fn press(s: &mut Connect, codes: &[KeyCode]) -> Action {
        let mut last = Action::None;
        for c in codes {
            last = key(s, KeyEvent::from(*c));
        }
        last
    }

    fn typed(s: &mut Connect, text: &str) {
        for c in text.chars() {
            key(s, KeyEvent::from(KeyCode::Char(c)));
        }
    }

    fn der(b64: &str) -> Vec<u8> {
        crate::tui::netprobe::tests::unbase64(b64)
    }

    /// `[leaf localhost, CA]`, as a server sending its chain presents it.
    pub(crate) fn fixture_chain() -> Vec<CertInfo> {
        vec![
            parse_cert(&der(include_str!(
                "../../../tests/fixtures/tls-leaf.der.b64"
            )))
            .unwrap(),
            parse_cert(&der(include_str!("../../../tests/fixtures/tls-ca.der.b64"))).unwrap(),
        ]
    }

    fn found() -> Vec<Found> {
        vec![
            Found {
                origin: "recent".into(),
                name: "logistics".into(),
                version: "17".into(),
                prefill: source::parse_any("postgres://oo_reader@db.internal:5432/logistics").unwrap(),
            },
            Found {
                origin: "~/.pgpass".into(),
                name: String::new(),
                version: String::new(),
                prefill: source::parse_any("postgres://ro:pw@10.0.4.12:5432/dw").unwrap(),
            },
            Found {
                origin: "tnsnames.ora".into(),
                name: "SALES".into(),
                version: String::new(),
                prefill: source::parse_any(
                    "(DESCRIPTION=(ADDRESS=(HOST=ora.internal)(PORT=1521))(CONNECT_DATA=(SERVICE_NAME=SALES)))",
                )
                .unwrap(),
            },
        ]
    }

    /// A PostgreSQL form after live checks and sign-in, as on loopback.
    pub(crate) fn signed_in() -> Connect {
        let mut s = Connect::new(found());
        s.open_engine(Engine::Postgres);
        s.host = "localhost".into();
        s.database = "logistics".into();
        s.password = "hunter2".into();
        s.env_set = true;
        let g = s.generation;
        s.apply(Outcome::Resolved(
            "localhost".into(),
            Ok(vec!["127.0.0.1".parse().unwrap()]),
        ));
        s.apply(Outcome::Port(
            "localhost".into(),
            5432,
            Ok(Duration::from_millis(2)),
        ));
        s.apply(Outcome::Tls(
            "localhost".into(),
            5432,
            Ok(TlsReport {
                chain: fixture_chain(),
                server_version: None,
            }),
        ));
        s.tls_mode = TlsMode::Trust;
        s.apply(Outcome::Verified(Verified {
            generation: g,
            auth: Ok(String::new()),
            checked: true,
            version: Some("17.11".into()),
            databases: Vec::new(),
        }));
        s.field = Field::Tls;
        s
    }

    #[test]
    fn pick_engine_then_form_has_defaults() {
        let mut s = Connect::new(Vec::new());
        assert_eq!(s.choices().len(), 4);
        press(&mut s, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
        assert_eq!((s.stage, s.engine), (Stage::Form, Engine::Oracle));
        assert_eq!(s.port_number(), Some(1521));
        assert_eq!(
            (s.database.as_str(), s.user.as_str()),
            ("ORCLPDB1", "OO_READER")
        );
        assert!(s.fields().contains(&Field::ConnectBy));
        assert!(!s.fields().contains(&Field::Tls));
        assert_eq!(s.field, Field::Host);
        typed(&mut s, "ora");
        assert_eq!(s.host, "ora");
        // ⏎ skips fields that hold good defaults and lands on the password.
        press(&mut s, &[KeyCode::Enter]);
        assert_eq!(s.field, Field::Password);
        // ←→ on the service row walks known services.
        s.field = Field::Database;
        press(&mut s, &[KeyCode::Right]);
        assert_eq!(s.database, "FREEPDB1");
        s.field = Field::ConnectBy;
        press(&mut s, &[KeyCode::Right]);
        assert!(s.sid);
        press(&mut s, &[KeyCode::Esc]);
        assert_eq!(s.stage, Stage::Pick);
    }

    #[test]
    fn paste_parses_and_fills_without_echoing_the_password() {
        let mut s = Connect::new(Vec::new());
        paste(&mut s, "jdbc:oracle:thin:scott/tiger@ora:1521:ORCL");
        assert!(source::mask(&s.paste).contains(MASK));
        press(&mut s, &[KeyCode::Enter]);
        assert_eq!(
            (s.stage, s.engine, s.sid),
            (Stage::Form, Engine::Oracle, true)
        );
        assert_eq!(s.take_password().as_deref(), Some("tiger"));
        let mut t = Connect::new(Vec::new());
        typed(&mut t, "nonsense words");
        press(&mut t, &[KeyCode::Enter]);
        assert_eq!(t.stage, Stage::Pick);
        assert!(t.error.is_some());
    }

    #[test]
    fn live_jobs_follow_the_form_and_enter_signs_in_then_continues() {
        let mut s = Connect::new(Vec::new());
        s.open_engine(Engine::Postgres);
        assert_eq!(s.field, Field::Host);
        typed(&mut s, "localhost");
        assert_eq!(s.host, "localhost", "typing replaces the suggested default");
        assert!(s.wants().is_empty(), "debounced while typing");
        s.frame += DEBOUNCE;
        assert!(matches!(s.wants()[..], [Job::Resolve(_)]));
        s.apply(Outcome::Resolved(
            "localhost".into(),
            Ok(vec!["127.0.0.1".parse().unwrap()]),
        ));
        assert!(matches!(s.wants()[..], [Job::Port(_, 5432)]));
        s.apply(Outcome::Port(
            "localhost".into(),
            5432,
            Ok(Duration::from_millis(1)),
        ));
        assert!(matches!(
            s.wants()[..],
            [Job::Tls(Engine::Postgres, _, 5432)]
        ));
        s.apply(Outcome::Tls(
            "localhost".into(),
            5432,
            Ok(TlsReport {
                chain: fixture_chain(),
                server_version: None,
            }),
        ));
        assert!(s.wants().is_empty());
        // ⏎ with no password jumps to it.
        press(&mut s, &[KeyCode::Enter]);
        assert_eq!(s.field, Field::Password);
        typed(&mut s, "pw");
        // ⏎ then asks to choose TLS; `t` trusts (loopback).
        press(&mut s, &[KeyCode::Enter]);
        assert_eq!(s.field, Field::Tls);
        typed(&mut s, "t");
        assert_eq!(s.tls_mode, TlsMode::Trust);
        assert_eq!(press(&mut s, &[KeyCode::Enter]), Action::None);
        let Some(Job::Verify(req)) = s.take_verify() else {
            panic!("expected a sign-in job")
        };
        assert!(
            req.ca_pem
                .as_deref()
                .is_some_and(|p| p.contains("BEGIN CERTIFICATE"))
        );
        assert!(!format!("{req:?}").contains("\"pw\""));
        s.apply(Outcome::Verified(Verified {
            generation: req.generation,
            auth: Ok(String::new()),
            checked: true,
            version: Some("16.4".into()),
            databases: vec!["postgres".into(), "sales".into()],
        }));
        assert_eq!(version_label(&s), "16");
        assert!(s.db_options.contains(&"sales".to_owned()));
        assert_eq!(press(&mut s, &[KeyCode::Enter]), Action::Next);
        // Any edit invalidates the sign-in.
        s.field = Field::User;
        typed(&mut s, "x");
        assert!(!s.ready());
    }

    #[test]
    fn public_servers_are_never_trusted_with_one_key() {
        let mut s = signed_in();
        s.tls_mode = TlsMode::Auto;
        s.apply(Outcome::Resolved(
            "localhost".into(),
            Ok(vec!["8.8.8.8".parse().unwrap()]),
        ));
        typed(&mut s, "t");
        assert_eq!(s.tls_mode, TlsMode::Auto);
        press(&mut s, &[KeyCode::Right]);
        assert_eq!(s.tls_mode, TlsMode::File);
    }

    #[test]
    fn password_modes_reference_instead_of_copying() {
        let mut s = Connect::new(Vec::new());
        s.open_engine(Engine::Mysql);
        s.field = Field::Password;
        press(&mut s, &[KeyCode::Right]);
        assert_eq!(s.pass_mode, PassMode::Env);
        assert_eq!(s.password_ref().as_deref(), Some("env:MYSQL_PWD"));
        press(&mut s, &[KeyCode::Right]);
        s.pass_file = "/run/secrets/db".into();
        assert_eq!(s.password_ref().as_deref(), Some("file:/run/secrets/db"));
        assert_eq!(s.take_password(), None);
    }

    #[test]
    fn snapshots() {
        let pick = Connect::new(found());
        snapshot::all_sizes("1-connect-pick", |area, buf, theme| {
            render(&pick, area, buf, theme)
        });
        snapshot::check(
            "1-connect-pick",
            120,
            40,
            Theme::MONO.zh(),
            |area, buf, theme| render(&pick, area, buf, theme),
        );
        let mut engines = Connect::new(Vec::new());
        engines.cursor = 2;
        snapshot::check(
            "1-connect-engines",
            80,
            24,
            Theme::MONO,
            |area, buf, theme| render(&engines, area, buf, theme),
        );
        let mut pasted = Connect::new(Vec::new());
        pasted.paste =
            "jdbc:postgresql://db.internal:5432/logistics?user=oo_reader&password=hunter2".into();
        snapshot::check(
            "1-connect-paste",
            80,
            24,
            Theme::MONO,
            |area, buf, theme| render(&pasted, area, buf, theme),
        );
        snapshot::check(
            "1-connect-paste",
            80,
            24,
            Theme::MONO.zh(),
            |area, buf, theme| render(&pasted, area, buf, theme),
        );
        let form = signed_in();
        snapshot::all_sizes("1-connect-form", |area, buf, theme| {
            render(&form, area, buf, theme)
        });
        snapshot::check(
            "1-connect-form",
            120,
            40,
            Theme::MONO.zh(),
            |area, buf, theme| render(&form, area, buf, theme),
        );
        let mut tls = signed_in();
        tls.tls_mode = TlsMode::Auto;
        tls.live.verified = None;
        snapshot::check("1-connect-tls", 80, 24, Theme::MONO, |area, buf, theme| {
            render(&tls, area, buf, theme)
        });
        snapshot::check(
            "1-connect-tls",
            80,
            24,
            Theme::MONO.zh(),
            |area, buf, theme| render(&tls, area, buf, theme),
        );
        let mut ora = Connect::new(Vec::new());
        ora.open(
            &source::parse_any("jdbc:oracle:thin:@ora.internal:1521/ORCLPDB1").unwrap(),
            "",
        );
        ora.field = Field::Version;
        ora.version = Some(1);
        snapshot::all_sizes("1-connect-oracle", |area, buf, theme| {
            render(&ora, area, buf, theme)
        });
        let text = {
            let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
            render(&form, buf.area, &mut buf, &Theme::MONO);
            render(&pasted, buf.area, &mut buf, &Theme::MONO);
            super::super::buffer_text(&buf)
        };
        assert!(!text.contains("hunter2"));
    }

    #[test]
    fn report_snapshot() {
        let r = report(
            "PostgreSQL localhost:5432 / logistics · oo_reader",
            vec![
                Check::new("source source", Status::Pass, ""),
                Check::new("access", Status::Warn, "missing SELECT on lg.audit_trail")
                    .fix("GRANT SELECT ON lg.audit_trail TO oo_reader;"),
                Check::new("model probe", Status::Pass, ""),
            ],
        );
        snapshot::check(
            "1-connect-report",
            80,
            24,
            Theme::MONO,
            |area, buf, theme| render(&r, area, buf, theme),
        );
    }

    #[test]
    fn live_check_frames() {
        // Host typed, then each live answer arriving a frame apart.
        let mut s = Connect::new(Vec::new());
        s.open_engine(Engine::Postgres);
        s.password = "pw".into();
        let steps: Vec<Connect> = {
            let mut out = vec![s.clone()];
            s.apply(Outcome::Resolved(
                "localhost".into(),
                Ok(vec!["127.0.0.1".parse().unwrap()]),
            ));
            out.push(s.clone());
            s.apply(Outcome::Port(
                "localhost".into(),
                5432,
                Ok(Duration::from_millis(1)),
            ));
            out.push(s.clone());
            s.apply(Outcome::Tls(
                "localhost".into(),
                5432,
                Ok(TlsReport {
                    chain: fixture_chain(),
                    server_version: None,
                }),
            ));
            out.push(s.clone());
            s.tls_mode = TlsMode::Trust;
            s.field = Field::Tls;
            s.live.verifying = Some(s.generation);
            out.push(s.clone());
            let g = s.generation;
            s.apply(Outcome::Verified(Verified {
                generation: g,
                auth: Ok(String::new()),
                checked: true,
                version: Some("17.11".into()),
                databases: Vec::new(),
            }));
            out.push(s);
            out
        };
        let frames: Vec<(usize, &str)> = vec![
            (0, "form opened: defaults dim, host resolving"),
            (1, "host resolved, port checking"),
            (2, "port open, reading the certificate"),
            (3, "certificate shown, trust or choose a CA"),
            (4, "t trusts the CA, ⏎ signs in"),
            (5, "signed in, version detected and picked"),
        ];
        snapshot::frames(
            "1-connect",
            80,
            24,
            false,
            &frames,
            |f, area, buf, theme| {
                let mut st = steps[f].clone();
                st.frame = f;
                render(&st, area, buf, theme);
            },
        );
    }
}
