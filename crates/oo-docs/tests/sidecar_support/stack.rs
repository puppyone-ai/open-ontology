//! A real `oo-docs-engine` process wired to a real Docling sidecar container,
//! shared by `l3_routing` and `faults` (`#[path]` include; both declare
//! `mod common;` and `mod sidecar_support;`).
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::common::engine::{ChildEngine, TOKEN};
use crate::common::{self, installed_db, pool_runtime};
use crate::sidecar_support::Sidecar;
use serde_json::{Value, json};
use sqlx::PgPool;

/// A real engine process with the sidecar wired in through the environment.
pub struct Stack {
    pub db: String,
    pub state: std::path::PathBuf,
    pub root: std::path::PathBuf,
    pub engine: Option<ChildEngine>,
    pub sidecar: Sidecar,
    pub pool: PgPool,
    /// Parser URL the engine gets instead of the sidecar's own address (a
    /// fault proxy in front of it).
    pub parser_url: Option<String>,
}

impl Stack {
    pub async fn start(db: &str, workers: u32) -> Stack {
        {
            let _g = common::CLUSTER_LOCK.lock().await;
            installed_db(db).await;
        }
        let state = std::env::temp_dir().join(format!("oo-l3-{db}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let root = state.join("drive");
        std::fs::create_dir_all(&root).unwrap();
        let sidecar = Sidecar::start(workers);
        std::fs::write(state.join("parser.token"), &sidecar.token).unwrap();
        let pool = pool_runtime(db, 4).await;
        let mut s = Stack {
            db: db.to_owned(),
            state,
            root,
            engine: None,
            sidecar,
            pool,
            parser_url: None,
        };
        s.spawn_engine(&[]);
        s
    }

    pub fn spawn_engine(&mut self, extra: &[(&str, &str)]) {
        let url = self
            .parser_url
            .clone()
            .unwrap_or_else(|| format!("http://{}", self.sidecar.addr));
        let token = self.state.join("parser.token");
        let mut env: Vec<(&str, &str)> = vec![
            ("OO_DOCS_PARSER_URL", url.as_str()),
            ("OO_DOCS_PARSER_TOKEN_FILE", token.to_str().unwrap()),
        ];
        env.extend_from_slice(extra);
        self.engine = Some(ChildEngine::spawn(
            env!("CARGO_BIN_EXE_oo-docs-engine"),
            &self.db,
            &self.state,
            &env,
        ));
    }

    pub fn addr(&self) -> std::net::SocketAddr {
        self.engine.as_ref().unwrap().addr
    }

    pub async fn call(&self, method: &str, path: &str, body: Option<&Value>) -> common::http::Resp {
        let auth = format!("Bearer {TOKEN}");
        let key = common::engine::key();
        let mut headers = vec![("Authorization", auth.as_str())];
        if method == "POST" {
            headers.push(("Idempotency-Key", key.as_str()));
        }
        let text = body.map(Value::to_string);
        common::http::request(self.addr(), method, path, &headers, text.as_deref()).await
    }

    pub async fn register_drive(&self) {
        let r = self
            .call(
                "POST",
                "/v1/docs/drives",
                Some(&json!({"name": "l3", "backend": "fs",
                    "config": {"root": self.root.to_str().unwrap()}})),
            )
            .await;
        assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    }

    /// `path -> (state, code)` of the live files.
    pub async fn states(&self) -> BTreeMap<String, (String, Option<String>)> {
        let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT path, state, state_code FROM _oo_docs.file WHERE deleted_at IS NULL",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap();
        rows.into_iter().map(|(p, s, c)| (p, (s, c))).collect()
    }

    /// Wait until every live file is in a state that does not change by itself.
    pub async fn settled(
        &self,
        expect: usize,
        within: Duration,
    ) -> BTreeMap<String, (String, Option<String>)> {
        let t0 = Instant::now();
        loop {
            let st = self.states().await;
            let done = st.len() >= expect
                && st.values().all(|(s, _)| {
                    matches!(
                        s.as_str(),
                        "published"
                            | "duplicate"
                            | "unsupported"
                            | "encrypted"
                            | "parse_failed"
                            | "too_large"
                            | "extract_partial"
                    )
                });
            if done || t0.elapsed() > within {
                return st;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub async fn finish(mut self) {
        drop(self.engine.take());
        self.pool.close().await;
        let _ = std::fs::remove_dir_all(&self.state);
        common::drop_db(&self.db).await;
    }
}

pub fn copy_tree(from: &Path, to: &Path) -> usize {
    let mut n = 0;
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&dest).unwrap();
            n += copy_tree(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
            n += 1;
        }
    }
    n
}
