//! The docs engine process (10 §2.1): control API, scheduler, workers.
//!
//! Configuration is read from the environment and `<state>/docs.json`
//! (`oo_docs::config`). The process prints one JSON line,
//! `{"event":"listening","addr":"127.0.0.1:<port>"}`, once the API accepts
//! connections; the daemon waits for `/livez` anyway. Stops on SIGTERM or
//! SIGINT. Exit codes: 0 clean, 2 configuration, 3 database, 1 other.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use oo_docs::api::{self, AppState, FactorySyncPort, auth};
use oo_docs::config::EngineConfig;
use oo_docs::health::{DfProbe, HealthProbe};
use oo_docs::ingest::{AnyFactory, handlers, poll};
use oo_docs::jobs::scheduler::{Scheduler, SchedulerConfig};
use oo_docs::jobs::stages;
use oo_docs::obs;
use oo_docs::store::install;
use serde_json::json;
use sqlx::Connection;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
use std::str::FromStr;
use tokio::sync::watch;

fn main() -> ExitCode {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return ExitCode::from(1),
    };
    rt.block_on(run())
}

fn fail(code: u8, event: &str, why: impl std::fmt::Display) -> ExitCode {
    obs::log(
        "error",
        event,
        json!({ "reason": obs::scrub(&why.to_string()) }),
    );
    ExitCode::from(code)
}

async fn run() -> ExitCode {
    #[cfg(feature = "failpoints")]
    oo_docs::failpoints::init_from_env();
    let env = |k: &str| std::env::var(k).ok();
    let state = env("OO_DOCS_STATE_DIR").unwrap_or_default();
    let docs_json = std::fs::read_to_string(std::path::Path::new(&state).join("docs.json")).ok();
    let cfg = match EngineConfig::from_parts(&env, docs_json.as_deref()) {
        Ok(c) => c,
        Err(e) => return fail(2, "config.invalid", e),
    };

    let mut tokens = Vec::new();
    for file in &cfg.token_files {
        match std::fs::read_to_string(file) {
            Ok(t) if t.trim().len() >= 16 => tokens.push(auth::token_hash(t.trim())),
            _ => {
                return fail(
                    2,
                    "config.token_unreadable",
                    "token file missing or too short",
                );
            }
        }
    }

    // Install the schema when an administrator login is configured.
    if let Some(url) = &cfg.admin_database_url {
        let opts = match PgConnectOptions::from_str(url) {
            Ok(o) => o,
            Err(e) => return fail(2, "config.invalid", e),
        };
        let mut conn = match PgConnection::connect_with(&opts).await {
            Ok(c) => c,
            Err(e) => return fail(3, "db.connect_failed", e),
        };
        if let Err(e) = install::install(&mut conn).await {
            return fail(3, "db.install_failed", e);
        }
        let _ = conn.close().await;
    }

    let opts = match PgConnectOptions::from_str(&cfg.database_url) {
        Ok(o) => o,
        Err(e) => return fail(2, "config.invalid", e),
    };
    let pool = match PgPoolOptions::new()
        .max_connections(24)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(opts)
        .await
    {
        Ok(p) => p,
        Err(e) => return fail(3, "db.connect_failed", e),
    };
    let schema_versions = {
        let mut conn = match pool.acquire().await {
            Ok(c) => c,
            Err(e) => return fail(3, "db.connect_failed", e),
        };
        match install::verify_readiness(&mut conn).await {
            Ok(v) => v.known.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
            Err(e) => return fail(3, "db.not_ready", format!("{e:?}")),
        }
    };

    let cas_root = std::path::Path::new(&cfg.state_dir).join("cas");
    let cas = match oo_docs::cas::Cas::open(&cas_root) {
        Ok(c) => Arc::new(c),
        Err(e) => return fail(1, "cas.open_failed", e),
    };

    let listener = match tokio::net::TcpListener::bind(cfg.listen).await {
        Ok(l) => l,
        Err(e) => return fail(1, "listen.failed", e),
    };
    let addr = match listener.local_addr() {
        Ok(a) => a,
        Err(e) => return fail(1, "listen.failed", e),
    };

    let disk: Arc<dyn oo_docs::health::DiskProbe> = Arc::new(DfProbe);
    let guard = Arc::new(oo_docs::obs::disk::Guard::new(
        disk.clone(),
        cas_root.clone(),
    ));
    let health = Arc::new(HealthProbe::new(
        pool.clone(),
        Some(cas_root),
        Box::new(disk),
        schema_versions,
    ));
    let limits = Arc::new(cfg.ingest.clone());
    let sync = Arc::new(FactorySyncPort::new(
        pool.clone(),
        AnyFactory::from_env(),
        cfg.ingest.clone(),
    ));
    let progress = oo_docs::api::progress::Bus::new();
    let app = Arc::new(
        AppState::new(
            pool.clone(),
            tokens,
            oo_docs::api::auth::allowed_hosts(addr.port(), &cfg.allowed_hosts),
            sync,
            health.clone(),
        )
        .with_cas(cas.clone())
        .with_progress(progress.clone()),
    );

    let (stop_tx, stop_rx) = watch::channel(false);
    let worker_id = oo_docs::jobs::worker_id(
        &std::env::var("HOSTNAME").unwrap_or_else(|_| "engine".into()),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64),
    );
    let mut sched_cfg = SchedulerConfig::new(&worker_id);
    sched_cfg.concurrency = stages::default_concurrency();
    sched_cfg.concurrency.extend(cfg.concurrency.clone());
    let sidecar = match &cfg.parser {
        Some(ep) => match std::fs::read_to_string(&ep.token_file) {
            Ok(t) if t.trim().len() >= 16 => {
                Some(Arc::new(oo_docs::parse::sidecar::SidecarClient::new(
                    oo_docs::parse::sidecar::SidecarConfig::new(ep.addr.clone(), t.trim()),
                )))
            }
            _ => {
                return fail(
                    2,
                    "config.parser_token_unreadable",
                    "parser token file missing or too short",
                );
            }
        },
        None => None,
    };
    let mut handlers = handlers::handlers(&pool, Arc::new(AnyFactory::from_env()), limits);
    handlers.extend(stages::handlers_with_sidecar(
        &pool,
        Arc::new(AnyFactory::from_env()),
        cas,
        Some(progress),
        sidecar,
    ));
    // Low disk pauses hashing, parsing and extraction (4.12, G7).
    oo_docs::obs::disk::guard_handlers(
        &mut handlers,
        &guard,
        &[
            oo_docs::jobs::JobKind::Hash,
            oo_docs::jobs::JobKind::Parse,
            oo_docs::jobs::JobKind::ExtractRules,
        ],
    );
    let scheduler = Scheduler::new(pool.clone(), sched_cfg, handlers).spawn();
    // Files made by an older parser or ruleset are reprocessed (4.2, D23).
    if let Ok(n) = stages::mark_outdated(&pool).await
        && n > 0
    {
        obs::log("info", "files.outdated", json!({ "count": n }));
    }
    let tasks = vec![
        stages::spawn_pump_guarded(
            pool.clone(),
            app.wake.clone(),
            stop_rx.clone(),
            Some(guard.clone()),
        ),
        poll::spawn(
            pool.clone(),
            cfg.poll.clone(),
            app.wake.clone(),
            stop_rx.clone(),
        ),
        api::events::spawn_hub(pool.clone(), app.events.clone(), stop_rx.clone()),
        api::freshness::spawn_watcher(pool.clone(), Duration::from_secs(1), stop_rx.clone()),
        api::lifecycle::spawn(pool.clone(), Duration::from_secs(1), stop_rx.clone()),
    ];
    health.set_ready();
    println!(
        "{}",
        json!({ "event": "listening", "addr": addr.to_string(), "worker": worker_id })
    );

    let shutdown = {
        let app = app.clone();
        async move {
            wait_for_signal().await;
            app.shutdown();
        }
    };
    let served = api::serve(listener, app, shutdown).await;
    let _ = stop_tx.send(true);
    scheduler.shutdown().await;
    for t in tasks {
        let _ = t.await;
    }
    pool.close().await;
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(1, "serve.failed", e),
    }
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
