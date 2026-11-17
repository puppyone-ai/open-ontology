//! A real API server over a real database for the HTTP tests.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use oo_docs::api::{self, AppState, FactorySyncPort, auth};
use oo_docs::cas::Cas;
use oo_docs::health::{DiskProbe, HealthProbe};
use oo_docs::ingest::{FsFactory, IngestLimits, handlers};
use oo_docs::jobs::JobKind;
use oo_docs::jobs::scheduler::{Scheduler, SchedulerConfig, SchedulerHandle};
use oo_docs::jobs::stages;
use oo_docs::obs::disk::Guard;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::http::{self, Resp};
use super::{CLUSTER_LOCK, drop_db, installed_db, pool_runtime};

pub const TOKEN: &str = "oodi_test_token_0123456789abcdef";

static KEYS: AtomicU64 = AtomicU64::new(1);

/// Fresh idempotency key.
pub fn key() -> String {
    format!("test-key-{:016}", KEYS.fetch_add(1, Ordering::Relaxed))
}

/// Disk probe reading a number the test changes.
pub struct SharedDisk(pub Arc<AtomicU64>);

impl DiskProbe for SharedDisk {
    fn free_bytes(&self, _: &std::path::Path) -> Option<u64> {
        Some(self.0.load(Ordering::Relaxed))
    }
}

/// Disk probe with a fixed answer.
pub struct FixedDisk(pub Option<u64>);

impl DiskProbe for FixedDisk {
    fn free_bytes(&self, _: &std::path::Path) -> Option<u64> {
        self.0
    }
}

#[derive(Default)]
pub struct Opts {
    /// Run the scheduler (sync and hash jobs).
    pub workers: bool,
    /// Run the freshness watcher.
    pub watcher: bool,
    /// Run the pipeline stages (parse, extract, publish), the pump and give
    /// the API its content store.
    pub stages: bool,
    /// Adjust the state before the server starts.
    pub tweak: Option<Tweak>,
    /// Free bytes the disk guard and health see (changeable by the test);
    /// `None` is 50 GiB and no guard.
    pub free_bytes: Option<Arc<AtomicU64>>,
}

/// Adjusts the state before the server starts.
pub type Tweak = Box<dyn FnOnce(&mut AppState)>;

pub struct TestEngine {
    pub name: String,
    pub pool: PgPool,
    pub addr: SocketAddr,
    pub app: Arc<AppState>,
    pub root: PathBuf,
    pub cas: PathBuf,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
    sched: Option<SchedulerHandle>,
}

impl TestEngine {
    pub async fn start(name: &str, opts: Opts) -> Self {
        {
            let _g = CLUSTER_LOCK.lock().await;
            installed_db(name).await;
        }
        let pool = pool_runtime(name, 24).await;
        let root = std::env::temp_dir().join(format!("oo-docs-api-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let cas = std::env::temp_dir().join(format!("oo-docs-cas-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&cas).unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let limits = IngestLimits {
            settle: Duration::ZERO,
            page_size: 3,
            ..IngestLimits::default()
        };
        let (disk_box, guard): (Box<dyn DiskProbe>, Option<Arc<Guard>>) = match &opts.free_bytes {
            Some(free) => {
                let probe = Arc::new(SharedDisk(free.clone()));
                (
                    Box::new(probe.clone()),
                    Some(Arc::new(Guard::new(probe, cas.clone()))),
                )
            }
            None => (Box::new(FixedDisk(Some(50 * 1024 * 1024 * 1024))), None),
        };
        let health = Arc::new(HealthProbe::new(
            pool.clone(),
            Some(cas.clone()),
            disk_box,
            vec!["docs-bootstrap/1".into()],
        ));
        health.set_ready();
        let sync = Arc::new(FactorySyncPort::new(
            pool.clone(),
            FsFactory,
            limits.clone(),
        ));
        let mut app = AppState::new(
            pool.clone(),
            vec![auth::token_hash(TOKEN)],
            auth::allowed_hosts(addr.port(), &[]),
            sync,
            health,
        );
        app.sse.poll = Duration::from_millis(100);
        if opts.stages {
            app.cas = Some(Arc::new(Cas::open(&cas).expect("cas")));
        }
        if let Some(t) = opts.tweak {
            t(&mut app);
        }
        let app = Arc::new(app);
        let (stop, stop_rx) = watch::channel(false);
        let mut tasks = vec![
            api::events::spawn_hub(pool.clone(), app.events.clone(), stop_rx.clone()),
            tokio::spawn({
                let app = app.clone();
                let mut rx = stop_rx.clone();
                async move {
                    let _ = api::serve(listener, app, async move {
                        let _ = rx.changed().await;
                    })
                    .await;
                }
            }),
        ];
        if opts.stages {
            tasks.push(oo_docs::jobs::stages::spawn_pump_guarded(
                pool.clone(),
                app.wake.clone(),
                stop_rx.clone(),
                guard.clone(),
            ));
        }
        if opts.watcher {
            tasks.push(api::freshness::spawn_watcher(
                pool.clone(),
                Duration::from_millis(100),
                stop_rx.clone(),
            ));
        }
        let sched = opts.workers.then(|| {
            let mut cfg = SchedulerConfig::new("api-test/1/00000000000000bb");
            cfg.poll_interval = Duration::from_millis(50);
            cfg.concurrency.insert(JobKind::Hash, 3);
            cfg.concurrency.insert(JobKind::Sync, 1);
            let mut hs = handlers::handlers(&pool, Arc::new(FsFactory), Arc::new(limits));
            if let Some(cas) = &app.cas {
                cfg.concurrency.extend(stages::default_concurrency());
                hs.extend(stages::handlers_with_progress(
                    &pool,
                    Arc::new(FsFactory),
                    cas.clone(),
                    Some(app.progress.clone()),
                ));
            }
            if let Some(g) = &guard {
                oo_docs::obs::disk::guard_handlers(
                    &mut hs,
                    g,
                    &[JobKind::Hash, JobKind::Parse, JobKind::ExtractRules],
                );
            }
            Scheduler::new(pool.clone(), cfg, hs).spawn()
        });
        Self {
            name: name.to_owned(),
            pool,
            addr,
            app,
            root,
            cas,
            stop,
            tasks,
            sched,
        }
    }

    /// Authenticated request.
    pub async fn req(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Resp {
        let auth = format!("Bearer {TOKEN}");
        let mut h = vec![("Authorization", auth.as_str())];
        h.extend_from_slice(headers);
        let text = body.map(Value::to_string);
        http::request(self.addr, method, path, &h, text.as_deref()).await
    }

    pub async fn get(&self, path: &str) -> Resp {
        self.req("GET", path, &[], None).await
    }

    /// POST with a fresh idempotency key.
    pub async fn post(&self, path: &str, body: &Value) -> Resp {
        let k = key();
        self.req("POST", path, &[("Idempotency-Key", &k)], Some(body))
            .await
    }

    pub fn write(&self, rel: &str, content: &str) {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    pub fn rm(&self, rel: &str) {
        std::fs::remove_file(self.root.join(rel)).unwrap();
    }

    /// Create a drive over `self.root`; returns the Drive JSON.
    pub async fn create_drive(&self, name: &str) -> Value {
        let r = self
            .post(
                "/v1/docs/drives",
                &json!({"name": name, "backend": "fs", "config": {"root": self.root.to_str().unwrap()}}),
            )
            .await;
        assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
        r.json()
    }

    pub fn wake(&self) {
        if let Some(s) = &self.sched {
            s.wake();
        }
        self.app.wake.notify_waiters();
    }

    /// Poll until `f` returns true (50 ms steps, 20 s).
    pub async fn until<F, Fut>(&self, what: &str, mut f: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        for _ in 0..400 {
            if f().await {
                return;
            }
            self.wake();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("timed out waiting for {what}");
    }

    pub async fn finish(mut self) {
        self.app.shutdown();
        let _ = self.stop.send(true);
        if let Some(s) = self.sched.take() {
            s.shutdown().await;
        }
        for t in self.tasks.drain(..) {
            let _ = t.await;
        }
        self.pool.close().await;
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_dir_all(&self.cas);
        drop_db(&self.name).await;
    }
}

/// The real `oo-docs-engine` binary as a child process (not available to
/// tests of other crates: `CARGO_BIN_EXE_` is only set for this package).
pub struct ChildEngine {
    pub child: std::process::Child,
    pub addr: std::net::SocketAddr,
    pub stderr_path: PathBuf,
}

impl Drop for ChildEngine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl ChildEngine {
    /// Start it against database `db` (already installed, runtime login),
    /// `state` as its state dir (holding `engine.token` with [`TOKEN`]),
    /// extra environment pairs, stderr appended to `<state>/engine.stderr`.
    pub fn spawn(bin: &str, db: &str, state: &std::path::Path, env: &[(&str, &str)]) -> Self {
        use std::io::BufRead;
        let port = std::env::var("OO_DOCS_TEST_PG_PORT").expect("OO_DOCS_TEST_PG_PORT");
        let host = std::env::var("OO_DOCS_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let url = format!(
            "postgres://{}@{host}:{port}/{db}?sslmode=disable",
            super::RUNTIME_LOGIN
        );
        std::fs::write(state.join("engine.token"), TOKEN).unwrap();
        let stderr_path = state.join("engine.stderr");
        let stderr = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stderr_path)
            .unwrap();
        let mut cmd = std::process::Command::new(bin);
        cmd.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("OO_DOCS_LISTEN", "127.0.0.1:0")
            .env("OO_DOCS_STATE_DIR", state)
            .env("OO_DOCS_DATABASE_URL", url)
            .env("OO_DOCS_ENGINE_TOKEN_FILE", state.join("engine.token"))
            .stdout(std::process::Stdio::piped())
            .stderr(stderr);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("spawn engine");
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.as_mut().unwrap())
            .read_line(&mut line)
            .expect("first line");
        let v: Value =
            serde_json::from_str(line.trim()).unwrap_or_else(|_| panic!("first line: {line}"));
        Self {
            child,
            addr: v["addr"].as_str().expect("addr").parse().expect("addr"),
            stderr_path,
        }
    }

    /// SIGTERM (via `kill`), then wait for a clean exit.
    pub fn stop(mut self) -> std::process::ExitStatus {
        let _ = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(self.child.id().to_string())
            .status();
        self.child.wait().expect("wait")
    }
}
