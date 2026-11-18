//! `oo serve` (10 §8.6): single instance, private state, tokens, engine,
//! loopback proxy, clean shutdown.
//!
//! Exit codes: 0 normal, 2 arguments, 3 Docker unavailable (compose runtime),
//! 4 engine binary missing (host runtime) or image missing or digest mismatch
//! (compose runtime), 5 state directory permissions, 6 port in use, 7 engine
//! start timeout, 1 anything else.
//!
//! `--runtime host` (default) runs the engine as a child process (L2);
//! `--runtime compose` runs `docs-db`, `docs-engine` and `doc-parser` as the
//! compose project of `deploy/docs` (see `compose.rs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio::sync::watch;

use super::compose;
use super::engine::{self, EngineSpec, Supervisor};
use super::proxy::{self, DriveMounts, EngineLink, ProxyConfig};
use super::secrets::{self, LockResult};

/// Exit codes of 10 §8.6 point 9.
pub mod exit {
    pub const OK: u8 = 0;
    pub const OTHER: u8 = 1;
    pub const ARGS: u8 = 2;
    /// Docker (or `docker compose`) is not available; compose runtime only.
    pub const DOCKER: u8 = 3;
    pub const ENGINE_MISSING: u8 = 4;
    pub const STATE_DIR: u8 = 5;
    pub const PORT_IN_USE: u8 = 6;
    pub const ENGINE_TIMEOUT: u8 = 7;
}

/// What the invocation asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Run,
    Status,
    Stop,
}

/// Where the engine runs.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RuntimeKind {
    Host,
    Compose,
}

/// Parsed `oo serve` arguments.
#[derive(Debug, PartialEq, Eq)]
pub struct ServeArgs {
    pub runtime: RuntimeKind,
    pub deploy_dir: Option<PathBuf>,
    pub action: Action,
    pub state_dir: Option<PathBuf>,
    pub port: u16,
    pub keep_engine: bool,
    pub json: bool,
}

/// Parse the arguments after `serve`.
pub fn parse_args(rest: &[String]) -> Result<ServeArgs, String> {
    let mut a = ServeArgs {
        runtime: RuntimeKind::Host,
        deploy_dir: None,
        action: Action::Run,
        state_dir: None,
        port: 0,
        keep_engine: false,
        json: false,
    };
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_owned())),
            _ => (arg.as_str(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| it.next().cloned())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag {
            "--state-dir" => a.state_dir = Some(PathBuf::from(value("--state-dir")?)),
            "--port" => {
                a.port = value("--port")?
                    .parse()
                    .map_err(|_| "--port must be 0..=65535".to_owned())?;
            }
            "--runtime" => {
                a.runtime = match value("--runtime")?.as_str() {
                    "host" => RuntimeKind::Host,
                    "compose" => RuntimeKind::Compose,
                    other => return Err(format!("--runtime must be host or compose, not {other}")),
                };
            }
            "--deploy-dir" => a.deploy_dir = Some(PathBuf::from(value("--deploy-dir")?)),
            "--keep-engine" => a.keep_engine = true,
            "--json" => a.json = true,
            "--status" => a.action = Action::Status,
            "--stop" => a.action = Action::Stop,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(a)
}

fn resolve_state(args: &ServeArgs) -> Result<PathBuf, String> {
    args.state_dir
        .clone()
        .or_else(|| std::env::var_os("OO_DOCS_STATE_DIR").map(PathBuf::from))
        .or_else(secrets::default_state_dir)
        .ok_or_else(|| "no state directory: set --state-dir or HOME".to_owned())
}

fn say(args: &ServeArgs, human: String, machine: &Value) {
    println!(
        "{}",
        if args.json {
            machine.to_string()
        } else {
            human
        }
    );
}

/// Entry point; returns the process exit code.
pub fn run(rest: &[String]) -> u8 {
    let args = match parse_args(rest) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("oo serve: {e}");
            return exit::ARGS;
        }
    };
    let state = match resolve_state(&args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("oo serve: {e}");
            return exit::ARGS;
        }
    };
    if let Err(e) = secrets::ensure_state_dir(&state) {
        eprintln!("oo serve: {}", e.0);
        return exit::STATE_DIR;
    }
    match args.action {
        Action::Status => status(&args, &state),
        Action::Stop => stop(&state),
        Action::Run => start(&args, &state),
    }
}

fn status(args: &ServeArgs, state: &Path) -> u8 {
    if secrets::is_running(state)
        && let Some(v) = secrets::read_serve_json(state)
    {
        say(
            args,
            format!("oo serve is running (pid {}, port {})", v["pid"], v["port"]),
            &v,
        );
        return exit::OK;
    }
    say(
        args,
        "oo serve is not running".to_owned(),
        &json!({"protocol": secrets::SERVE_PROTOCOL, "running": false}),
    );
    exit::OTHER
}

fn stop(state: &Path) -> u8 {
    let Some(v) = secrets::read_serve_json(state).filter(|_| secrets::is_running(state)) else {
        println!("oo serve is not running");
        return exit::OK;
    };
    let Some(pid) = v["pid"].as_u64().and_then(|p| u32::try_from(p).ok()) else {
        eprintln!("oo serve: serve.json has no pid");
        return exit::OTHER;
    };
    engine::signal(pid, "TERM");
    // Stopping the compose runtime stops three containers (docker's own grace
    // period each), so the wait is longer than for a host process.
    for _ in 0..900 {
        if !secrets::is_running(state) {
            println!("oo serve stopped");
            return exit::OK;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("oo serve: did not stop within 90 s");
    exit::OTHER
}

fn start(args: &ServeArgs, state: &Path) -> u8 {
    let _lock = match secrets::acquire_lock(state) {
        Ok(LockResult::Held(l)) => l,
        Ok(LockResult::Busy) => {
            // Already running: print its serve.json so the caller reuses it.
            return match secrets::read_serve_json(state) {
                Some(v) => {
                    println!("{v}");
                    exit::OK
                }
                None => {
                    eprintln!("oo serve: another instance is starting");
                    exit::OTHER
                }
            };
        }
        Err(_) => {
            eprintln!("oo serve: serve.lock is not usable");
            return exit::STATE_DIR;
        }
    };
    // A crashed predecessor left files and maybe an engine behind.
    engine::reap_stale(&state.join("engine.pid"));
    secrets::clean_runtime_files(state);
    if args.runtime == RuntimeKind::Compose {
        return start_compose(args, state);
    }

    let Some(exe) = engine::find_engine_binary() else {
        eprintln!(
            "oo serve: the docs engine binary ({}) was not found; set OO_DOCS_ENGINE_BIN",
            engine::ENGINE_BIN
        );
        return exit::ENGINE_MISSING;
    };
    let Ok(database_url) = std::env::var("OO_DOCS_DATABASE_URL") else {
        eprintln!(
            "oo serve: OO_DOCS_DATABASE_URL is required (L2: Postgres is external to oo serve)"
        );
        return exit::OTHER;
    };
    if args.keep_engine {
        eprintln!("oo serve: --keep-engine has no effect while the engine is a host process");
    }
    if args.deploy_dir.is_some() {
        eprintln!("oo serve: --deploy-dir only applies to --runtime compose");
    }

    let (control, engine_token) = match (secrets::new_token("oodt_"), secrets::new_token("oodi_")) {
        (Ok(c), Ok(e)) => (c, e),
        _ => {
            eprintln!("oo serve: no randomness source");
            return exit::OTHER;
        }
    };
    let engine_dir = state.join("engine");
    let _ = secrets::ensure_state_dir(&engine_dir);
    let engine_token_file = state.join("engine.token");
    let token_file = state.join("serve.token");
    if secrets::write_secret_file(&engine_token_file, &engine_token).is_err()
        || secrets::write_secret_file(&token_file, &control).is_err()
    {
        eprintln!("oo serve: cannot write token files");
        secrets::clean_runtime_files(state);
        return exit::STATE_DIR;
    }
    let spec = EngineSpec {
        exe,
        state_dir: engine_dir,
        database_url,
        admin_database_url: std::env::var("OO_DOCS_ADMIN_DATABASE_URL").ok(),
        token_file: engine_token_file,
        log_file: state.join("engine.log"),
    };
    let first = match engine::start_once(&spec) {
        Ok(f) => f,
        Err(why) => {
            eprintln!("oo serve: {why}");
            secrets::clean_runtime_files(state);
            return if why.contains("in time") {
                exit::ENGINE_TIMEOUT
            } else {
                exit::OTHER
            };
        }
    };

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return exit::OTHER,
    };
    let code = rt.block_on(serve_loop(
        args,
        state,
        &token_file,
        &control,
        &engine_token,
        Backend::Host(spec, first),
    ));
    secrets::clean_runtime_files(state);
    code
}

/// The compose runtime start (10 §8.6 point 3): checks, files, containers,
/// then the same loopback proxy as the host runtime.
fn start_compose(args: &ServeArgs, state: &Path) -> u8 {
    let (Ok(control), Ok(engine_token), Ok(parser_token)) = (
        secrets::new_token("oodt_"),
        secrets::new_token("oodi_"),
        secrets::new_token("oodp_"),
    ) else {
        eprintln!("oo serve: no randomness source");
        return exit::OTHER;
    };
    let runtime = match compose::prepare(
        state,
        args.deploy_dir.as_deref(),
        &engine_token,
        &parser_token,
    ) {
        Ok(r) => Arc::new(r),
        Err(f) => {
            eprintln!("oo serve: {}", f.message);
            secrets::clean_runtime_files(state);
            return f.exit;
        }
    };
    let token_file = state.join("serve.token");
    if secrets::write_secret_file(&token_file, &control).is_err() {
        eprintln!("oo serve: cannot write token files");
        secrets::clean_runtime_files(state);
        return exit::STATE_DIR;
    }
    if let Err(f) = runtime.up() {
        eprintln!("oo serve: {}", f.message);
        runtime.stop(false);
        secrets::clean_runtime_files(state);
        return f.exit;
    }
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => {
            runtime.stop(false);
            return exit::OTHER;
        }
    };
    let code = rt.block_on(serve_loop(
        args,
        state,
        &token_file,
        &control,
        &engine_token,
        Backend::Compose(runtime),
    ));
    secrets::clean_runtime_files(state);
    code
}

/// What runs the engine.
enum Backend {
    Host(EngineSpec, (std::process::Child, std::net::SocketAddr)),
    Compose(Arc<compose::Runtime>),
}

async fn serve_loop(
    args: &ServeArgs,
    state: &Path,
    token_file: &Path,
    control: &str,
    engine_token: &str,
    backend: Backend,
) -> u8 {
    let listener = match TcpListener::bind(("127.0.0.1", args.port)).await {
        Ok(l) => l,
        Err(_) => {
            match backend {
                Backend::Host(_, mut first) => {
                    let _ = first.0.kill();
                    let _ = first.0.wait();
                }
                Backend::Compose(rt) => rt.stop(false),
            }
            eprintln!("oo serve: port {} is in use", args.port);
            return exit::PORT_IN_USE;
        }
    };
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(_) => return exit::OTHER,
    };
    let link = Arc::new(EngineLink::default());
    let (supervisor, compose_rt, engine_json, drives) = match backend {
        Backend::Host(spec, first) => {
            let pid = first.0.id();
            let sup = Supervisor::spawn(spec, first, link.clone(), state.join("engine.pid"));
            (
                Some(sup),
                None,
                json!({"mode": "host_process", "pid": pid, "project": "host", "images": {}}),
                None,
            )
        }
        Backend::Compose(rt) => {
            link.set(Some(rt.engine_addr()));
            let drives: Arc<dyn DriveMounts> = rt.clone();
            (None, Some(rt.clone()), rt.describe(), Some(drives))
        }
    };
    let cfg = Arc::new(ProxyConfig {
        port,
        token_sha256: Sha256::digest(control.as_bytes()).into(),
        engine_token: engine_token.to_owned(),
        engine: link,
        drives,
    });
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let doc = secrets::serve_json(
        std::process::id(),
        port,
        token_file,
        state,
        &secrets::rfc3339(started),
        engine_json,
    );
    if secrets::write_secret_atomic(&state.join("serve.json"), &doc.to_string()).is_err() {
        stop_backend(supervisor, compose_rt, false, Duration::from_secs(5)).await;
        eprintln!("oo serve: cannot write serve.json");
        return exit::STATE_DIR;
    }
    say(
        args,
        format!(
            "oo serve listening on 127.0.0.1:{port} (state {})",
            state.display()
        ),
        &doc,
    );

    let (stop_tx, stop_rx) = watch::channel(false);
    let reconciler = tokio::spawn(reconcile_loop(state.to_owned(), stop_rx.clone()));
    let server = tokio::spawn(proxy::serve(
        listener,
        cfg,
        stop_rx,
        Duration::from_secs(10),
    ));
    wait_for_signal().await;
    // Stop accepting, drain in-flight requests, then remove the secrets
    // before the engine goes (10 §8.6 point 7).
    let _ = stop_tx.send(true);
    let _ = server.await;
    let _ = reconciler.await;
    secrets::clean_runtime_files(state);
    stop_backend(
        supervisor,
        compose_rt,
        args.keep_engine,
        Duration::from_secs(10),
    )
    .await;
    println!("oo serve stopped");
    exit::OK
}

/// The daemon's mount upkeep (10 §7.7 point 6, §16.4): every second, delete the
/// directories of expired and revoked shares and append new `UPDATES.md`
/// entries. Quiet when the engine is down; it simply tries again.
async fn reconcile_loop(state: PathBuf, mut stop: watch::Receiver<bool>) {
    loop {
        if *stop.borrow() {
            return;
        }
        if !super::mounts::load_mounts(&state).is_empty()
            && let Ok(d) = super::client::Daemon::connect(&state)
        {
            let _ = super::mounts::reconcile(&d, &state).await;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            _ = stop.changed() => {}
        }
    }
}

/// Stop the engine side: the host child, or `docker compose stop` (volumes
/// stay; `--keep-engine` leaves the containers running).
async fn stop_backend(
    supervisor: Option<Supervisor>,
    compose_rt: Option<Arc<compose::Runtime>>,
    keep_engine: bool,
    grace: Duration,
) {
    let _ = tokio::task::spawn_blocking(move || {
        if let Some(s) = supervisor {
            s.stop(grace);
        }
        if let Some(rt) = compose_rt {
            rt.stop(keep_engine);
        }
    })
    .await;
}

async fn wait_for_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut term), Ok(mut int)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        std::future::pending::<()>().await;
        return;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn arguments_parse() {
        let p = parse_args(&a(&["--state-dir", "/s", "--port=8123", "--json"])).unwrap();
        assert_eq!(p.state_dir, Some(PathBuf::from("/s")));
        assert_eq!((p.port, p.json, p.action), (8123, true, Action::Run));
        assert_eq!(
            parse_args(&a(&["--status"])).unwrap().action,
            Action::Status
        );
        assert_eq!(parse_args(&a(&["--stop"])).unwrap().action, Action::Stop);
        assert!(parse_args(&a(&["--keep-engine"])).unwrap().keep_engine);
        assert_eq!(parse_args(&a(&[])).unwrap().port, 0);
    }

    #[test]
    fn the_runtime_defaults_to_host_and_compose_is_opt_in() {
        let d = parse_args(&a(&[])).unwrap();
        assert_eq!((d.runtime, d.deploy_dir), (RuntimeKind::Host, None));
        let c = parse_args(&a(&["--runtime", "compose", "--deploy-dir=/d"])).unwrap();
        assert_eq!(c.runtime, RuntimeKind::Compose);
        assert_eq!(c.deploy_dir, Some(PathBuf::from("/d")));
        assert!(parse_args(&a(&["--runtime", "podman"])).is_err());
        assert!(parse_args(&a(&["--runtime"])).is_err());
    }

    #[test]
    fn bad_arguments_are_refused() {
        for bad in [
            &["--port"][..],
            &["--port", "70000"],
            &["--port=x"],
            &["--state-dir"],
            &["--nope"],
            &["stray"],
        ] {
            assert!(parse_args(&a(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn exit_codes_are_the_documented_ones() {
        assert_eq!(
            [
                exit::ARGS,
                exit::ENGINE_MISSING,
                exit::STATE_DIR,
                exit::PORT_IN_USE,
                exit::ENGINE_TIMEOUT
            ],
            [2, 4, 5, 6, 7]
        );
    }

    #[test]
    fn a_loose_state_dir_exits_5() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("oo-serve-loose-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let code = run(&a(&["--state-dir", dir.to_str().unwrap()]));
        assert_eq!(code, exit::STATE_DIR);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_of_a_fresh_state_dir_is_not_running() {
        let dir = std::env::temp_dir().join(format!("oo-serve-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            run(&a(&["--status", "--state-dir", dir.to_str().unwrap()])),
            exit::OTHER
        );
        assert_eq!(
            run(&a(&["--stop", "--state-dir", dir.to_str().unwrap()])),
            exit::OK
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
