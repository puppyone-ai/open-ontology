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
        let health = Arc::new(HealthProbe::new(
            pool.clone(),
            Some(cas.clone()),
            Box::new(FixedDisk(Some(50 * 1024 * 1024 * 1024))),
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
            tasks.push(oo_docs::jobs::stages::spawn_pump(
                pool.clone(),
                app.wake.clone(),
                stop_rx.clone(),
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
                hs.extend(stages::handlers(&pool, Arc::new(FsFactory), cas.clone()));
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
