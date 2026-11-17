//! `oo doctor`, section `docs` (10 §10.4, board WS-BE5, B-S4-05).
//!
//! Feature `docs`. The section is `{"status", "checks": [...]}` and is added
//! to the `oo.doctor/v1` report as a new field. `not_configured` (no state
//! directory: Super Drive never started on this machine) is not a failure.
//! Every check has a `status` of `ok`, `failed`, `not_applicable`,
//! `not_running`, `not_configured` or `not_covered`; only `failed` blocks.
//!
//! What the CLI can see it checks itself (state directory, token files,
//! drive roots); what only the engine knows comes from `/v1/docs/health` and
//! `/v1/docs/drives` over the loopback proxy, so this crate still does not
//! link the engine. L2 reading of the table: the engine is a host process, so
//! `docker` and `images` do not apply while `serve.json` says
//! `host_process` (or no engine runs); `mounts` is `not_covered` because this
//! build writes no `mounts.json`.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::docs::client::Daemon;
use crate::docs::secrets;

/// Free space below which the disk check fails (10 §10.4, 2 GiB).
pub const DISK_LOW_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// State directory facts.
#[derive(Clone, Debug)]
pub struct DirFacts {
    pub mode: u32,
    pub owned_by_me: bool,
    pub symlink: bool,
    /// The engine's own subdirectory is a symlink.
    pub engine_dir_symlink: bool,
}

/// One token file.
#[derive(Clone, Debug)]
pub struct FileFacts {
    pub name: &'static str,
    pub mode: u32,
    pub nlink: u64,
    pub symlink: bool,
}

/// What the engine said.
#[derive(Clone, Debug)]
pub enum Engine {
    /// No `serve.json`.
    NotRunning,
    /// `serve.json` exists and the engine does not answer (code and why).
    Unreachable(String),
    /// Answered.
    Up { health: Value, drives: Value },
}

/// A local drive root.
#[derive(Clone, Debug)]
pub struct Root {
    pub drive_id: String,
    pub readable: bool,
}

/// Everything the checks look at.
#[derive(Clone, Debug)]
pub struct Facts {
    /// `None`: no state directory.
    pub state_dir: Option<DirFacts>,
    pub files: Vec<FileFacts>,
    /// `engine.mode` of `serve.json`.
    pub engine_mode: Option<String>,
    pub engine: Engine,
    pub roots: Vec<Root>,
}

fn check(name: &str, status: &str, code: Option<&str>, detail: Value) -> Value {
    let mut v = json!({ "check": name, "status": status, "detail": detail });
    if let Some(c) = code {
        v["code"] = json!(c);
    }
    v
}

fn ok(name: &str, detail: Value) -> Value {
    check(name, "ok", None, detail)
}

fn failed(name: &str, code: &str, detail: Value) -> Value {
    check(name, "failed", Some(code), detail)
}

fn skipped(name: &str, status: &str, why: &str) -> Value {
    check(name, status, None, json!({ "why": why }))
}

/// All checks over `facts`. Pure.
pub fn evaluate(f: &Facts) -> Value {
    let Some(dir) = &f.state_dir else {
        return json!({
            "status": "not_configured",
            "why": "no Super Drive state directory on this machine",
            "checks": [],
        });
    };
    let mut checks = Vec::new();

    // docker and images: only for an engine that runs in containers.
    let containers = f
        .engine_mode
        .as_deref()
        .is_some_and(|m| m != "host_process");
    for name in ["docker", "images"] {
        checks.push(if containers {
            skipped(
                name,
                "not_covered",
                "container mode checks are not implemented in this build",
            )
        } else {
            skipped(name, "not_applicable", "the engine runs as a host process")
        });
    }

    checks.push(if dir.symlink || dir.engine_dir_symlink {
        failed("state_dir", "DOCS_DOCTOR_STATE_PERMS", json!({ "problem": "symbolic link" }))
    } else if dir.mode & 0o077 != 0 {
        failed(
            "state_dir",
            "DOCS_DOCTOR_STATE_PERMS",
            json!({ "problem": "group or others have access", "mode": format!("{:o}", dir.mode & 0o777) }),
        )
    } else if !dir.owned_by_me {
        failed("state_dir", "DOCS_DOCTOR_STATE_PERMS", json!({ "problem": "not owned by this user" }))
    } else {
        ok("state_dir", json!({ "mode": format!("{:o}", dir.mode & 0o777) }))
    });

    let bad: Vec<Value> = f
        .files
        .iter()
        .filter(|t| t.symlink || t.mode & 0o777 != 0o600 || t.nlink != 1)
        .map(|t| json!({ "file": t.name, "mode": format!("{:o}", t.mode & 0o777), "nlink": t.nlink, "symlink": t.symlink }))
        .collect();
    checks.push(if bad.is_empty() {
        ok("token_file", json!({ "checked": f.files.len() }))
    } else {
        failed(
            "token_file",
            "DOCS_DOCTOR_TOKEN_PERMS",
            json!({ "files": bad }),
        )
    });

    let (health, drives) = match &f.engine {
        Engine::NotRunning => {
            checks.push(skipped("engine", "not_running", "oo serve is not running"));
            for n in [
                "schema",
                "parser",
                "model",
                "disk",
                "drives",
                "cas_sample",
                "stuck_jobs",
                "quarantine",
            ] {
                checks.push(skipped(n, "not_running", "needs the engine"));
            }
            return finish(checks, &f.roots, None);
        }
        Engine::Unreachable(why) => {
            checks.push(failed(
                "engine",
                "DOCS_ENGINE_UNAVAILABLE",
                json!({ "why": why }),
            ));
            for n in [
                "schema",
                "parser",
                "model",
                "disk",
                "drives",
                "cas_sample",
                "stuck_jobs",
                "quarantine",
            ] {
                checks.push(skipped(n, "not_running", "needs the engine"));
            }
            return finish(checks, &f.roots, None);
        }
        Engine::Up { health, drives } => (health, drives),
    };

    let db_ok = health["components"]["db"]["status"] == "ok";
    checks.push(if health["status"] == "unavailable" || !db_ok {
        failed(
            "engine",
            "DOCS_ENGINE_DB_UNAVAILABLE",
            json!({ "status": health["status"] }),
        )
    } else {
        ok(
            "engine",
            json!({ "status": health["status"], "engine_version": health["engine_version"] }),
        )
    });

    let versions = health["schema_versions"].as_array().map_or(0, Vec::len);
    checks.push(if !db_ok {
        skipped("schema", "not_running", "the database does not answer")
    } else if versions == 0 {
        failed(
            "schema",
            "DOCS_ENGINE_SCHEMA_MISMATCH",
            json!({ "schema_versions": [] }),
        )
    } else {
        ok(
            "schema",
            json!({ "schema_versions": health["schema_versions"] }),
        )
    });

    checks.push(if health["components"]["parser"]["status"] == "ok" {
        ok("parser", health["components"]["parser"]["detail"].clone())
    } else {
        failed(
            "parser",
            "DOCS_DOCTOR_PARSER",
            json!({ "status": health["components"]["parser"]["status"] }),
        )
    });

    checks.push(match health["components"]["model"]["status"].as_str() {
        Some("not_configured") | None => skipped(
            "model",
            "not_configured",
            "no model configured (not a failure)",
        ),
        Some("ok") => ok("model", json!({})),
        Some(other) => failed(
            "model",
            "DOCS_ENGINE_MODEL_UNAVAILABLE",
            json!({ "status": other }),
        ),
    });

    let free = health["disk"]["free_bytes"].as_u64();
    let cas_code = health["components"]["cas"]["code"].as_str();
    checks.push(match free {
        Some(n) if n < DISK_LOW_BYTES => failed(
            "disk",
            "DOCS_ENGINE_DISK_LOW",
            json!({ "free_bytes": n, "need_bytes": DISK_LOW_BYTES }),
        ),
        Some(n) => ok("disk", json!({ "free_bytes": n })),
        None => skipped(
            "disk",
            "not_covered",
            "the engine could not read the free space",
        ),
    });

    // drives: the engine's view (degraded reason) plus the local root.
    let mut drive_problems = Vec::new();
    let items = drives["items"].as_array().cloned().unwrap_or_default();
    for d in &items {
        let fresh = &d["freshness"];
        if fresh["state"] == "degraded" {
            drive_problems.push(json!({
                "drive_id": d["id"],
                "code": fresh["reason_code"].as_str().unwrap_or("DOCS_SOURCE_UNAVAILABLE"),
            }));
        }
    }
    for r in &f.roots {
        if !r.readable {
            drive_problems
                .push(json!({ "drive_id": r.drive_id, "code": "DOCS_SOURCE_ROOT_MISSING" }));
        }
    }
    checks.push(if drive_problems.is_empty() {
        ok("drives", json!({ "drives": items.len() }))
    } else {
        let code = drive_problems[0]["code"]
            .as_str()
            .unwrap_or("DOCS_SOURCE_UNAVAILABLE")
            .to_owned();
        failed("drives", &code, json!({ "problems": drive_problems }))
    });

    let sample = &health["disk"]["cas_sample"];
    checks.push(
        match (sample["checked"].as_u64(), sample["corrupt"].as_u64()) {
            (Some(c), Some(0)) => ok("cas_sample", json!({ "checked": c })),
            (Some(c), Some(bad)) => failed(
                "cas_sample",
                "DOCS_ENGINE_CAS_CORRUPT",
                json!({ "checked": c, "corrupt": bad }),
            ),
            _ => skipped(
                "cas_sample",
                "not_covered",
                "the engine did not report a sample",
            ),
        },
    );
    let _ = cas_code;

    let stuck = health["queue"]["stuck"].as_u64().unwrap_or(0);
    checks.push(if stuck > 0 {
        failed(
            "stuck_jobs",
            "DOCS_DOCTOR_STUCK_JOBS",
            json!({ "stuck": stuck, "lease_expired_over_minutes": 5 }),
        )
    } else {
        ok("stuck_jobs", json!({ "stuck": 0 }))
    });
    checks.push(check(
        "quarantine",
        "ok",
        None,
        json!({ "quarantined_jobs": health["queue"]["quarantined"].as_u64().unwrap_or(0), "info": true }),
    ));
    finish(checks, &f.roots, Some(()))
}

fn finish(mut checks: Vec<Value>, _roots: &[Root], _up: Option<()>) -> Value {
    checks.push(skipped(
        "mounts",
        "not_covered",
        "this build writes no mounts.json",
    ));
    let blocked = checks.iter().any(|c| c["status"] == "failed");
    json!({ "status": if blocked { "blocked" } else { "ok" }, "checks": checks })
}

// ------------------------------------------------------------------ gather

fn file_facts(dir: &Path, name: &'static str) -> Option<FileFacts> {
    let md = std::fs::symlink_metadata(dir.join(name)).ok()?;
    Some(FileFacts {
        name,
        mode: md.mode(),
        nlink: md.nlink(),
        symlink: md.file_type().is_symlink(),
    })
}

/// Read the facts: local files first, then the engine over loopback.
pub async fn gather(state: &Path) -> Facts {
    let Ok(md) = std::fs::symlink_metadata(state) else {
        return Facts {
            state_dir: None,
            files: Vec::new(),
            engine_mode: None,
            engine: Engine::NotRunning,
            roots: Vec::new(),
        };
    };
    let me = current_uid();
    let state_dir = Some(DirFacts {
        mode: md.mode(),
        owned_by_me: md.uid() == me,
        symlink: md.file_type().is_symlink(),
        engine_dir_symlink: std::fs::symlink_metadata(state.join("engine"))
            .is_ok_and(|m| m.file_type().is_symlink()),
    });
    let files: Vec<FileFacts> = ["serve.token", "engine.token"]
        .into_iter()
        .filter_map(|n| file_facts(state, n))
        .collect();
    let serve = secrets::read_serve_json(state);
    let engine_mode = serve
        .as_ref()
        .and_then(|s| s["engine"]["mode"].as_str().map(str::to_owned));
    let mut roots = Vec::new();
    let engine = if serve.is_none() {
        Engine::NotRunning
    } else {
        match Daemon::connect(state) {
            Err(e) => Engine::Unreachable(e.message),
            Ok(d) => match d.get_json("/v1/docs/health").await {
                Err(e) => Engine::Unreachable(e.message),
                Ok(health) => {
                    let drives = d
                        .get_json("/v1/docs/drives?limit=1000")
                        .await
                        .unwrap_or(Value::Null);
                    for item in drives["items"].as_array().into_iter().flatten() {
                        if item["backend"] == "fs"
                            && let Some(root) = item["config"]["root"].as_str()
                        {
                            roots.push(Root {
                                drive_id: item["id"].as_str().unwrap_or_default().to_owned(),
                                readable: std::fs::read_dir(root).is_ok(),
                            });
                        }
                    }
                    Engine::Up { health, drives }
                }
            },
        }
    };
    Facts {
        state_dir,
        files,
        engine_mode,
        engine,
        roots,
    }
}

/// The effective uid, read as the owner of a file this process just created
/// (the crate forbids unsafe code, so `geteuid` is not called).
fn current_uid() -> u32 {
    let probe = std::env::temp_dir().join(format!(".oo-doctor-uid-{}", std::process::id()));
    let uid = std::fs::File::create(&probe)
        .and_then(|f| f.metadata())
        .map_or(u32::MAX, |m| m.uid());
    let _ = std::fs::remove_file(&probe);
    uid
}

/// The state directory `oo doctor` looks at: `OO_DOCS_STATE_DIR`, else the
/// default of `oo serve`.
pub fn state_dir() -> Option<PathBuf> {
    std::env::var_os("OO_DOCS_STATE_DIR")
        .map(PathBuf::from)
        .or_else(secrets::default_state_dir)
}

/// The `docs` section for the doctor report.
pub async fn section() -> Value {
    match state_dir() {
        Some(dir) => evaluate(&gather(&dir).await),
        None => evaluate(&Facts {
            state_dir: None,
            files: Vec::new(),
            engine_mode: None,
            engine: Engine::NotRunning,
            roots: Vec::new(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> DirFacts {
        DirFacts {
            mode: 0o40700,
            owned_by_me: true,
            symlink: false,
            engine_dir_symlink: false,
        }
    }

    fn token(name: &'static str) -> FileFacts {
        FileFacts {
            name,
            mode: 0o100600,
            nlink: 1,
            symlink: false,
        }
    }

    fn health() -> Value {
        json!({
            "status": "ok", "engine_version": "x", "schema_versions": ["docs-bootstrap/1"],
            "components": {
                "db": {"status": "ok"}, "parser": {"status": "ok", "detail": {"kind": "local"}},
                "model": {"status": "not_configured"}, "cas": {"status": "ok"},
            },
            "queue": {"queued": 0, "running": 0, "failed_manual": 0, "quarantined": 2, "blocked": 0, "stuck": 0},
            "disk": {"free_bytes": 50u64 * 1024 * 1024 * 1024, "cas_sample": {"checked": 20, "corrupt": 0}},
        })
    }

    fn up(health: Value, drives: Value) -> Facts {
        Facts {
            state_dir: Some(dir()),
            files: vec![token("serve.token"), token("engine.token")],
            engine_mode: Some("host_process".into()),
            engine: Engine::Up { health, drives },
            roots: vec![],
        }
    }

    fn status(report: &Value, name: &str) -> (String, Option<String>) {
        let c = report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["check"] == name)
            .unwrap_or_else(|| panic!("{name}"));
        (
            c["status"].as_str().unwrap().into(),
            c["code"].as_str().map(str::to_owned),
        )
    }

    #[test]
    fn a_healthy_machine_passes_every_check_of_the_table() {
        let r = evaluate(&up(health(), json!({"items": []})));
        assert_eq!(r["status"], "ok", "{r}");
        for name in [
            "docker",
            "images",
            "state_dir",
            "token_file",
            "engine",
            "schema",
            "parser",
            "model",
            "disk",
            "drives",
            "cas_sample",
            "stuck_jobs",
            "quarantine",
            "mounts",
        ] {
            let (s, _) = status(&r, name);
            assert!(
                matches!(
                    s.as_str(),
                    "ok" | "not_applicable" | "not_configured" | "not_covered"
                ),
                "{name}: {s}"
            );
        }
        assert_eq!(status(&r, "docker").0, "not_applicable");
        assert_eq!(
            status(&r, "model").0,
            "not_configured",
            "no model is not a failure"
        );
        assert_eq!(
            r["checks"].as_array().unwrap().len(),
            14,
            "every row of 10 4 .. 10.4"
        );
    }

    #[test]
    fn no_state_directory_is_not_a_failure() {
        let r = evaluate(&Facts {
            state_dir: None,
            files: vec![],
            engine_mode: None,
            engine: Engine::NotRunning,
            roots: vec![],
        });
        assert_eq!(r["status"], "not_configured");
    }

    #[test]
    fn a_stopped_engine_is_reported_but_does_not_block() {
        let mut f = up(Value::Null, Value::Null);
        f.engine = Engine::NotRunning;
        let r = evaluate(&f);
        assert_eq!(r["status"], "ok");
        assert_eq!(status(&r, "engine").0, "not_running");
    }

    #[test]
    fn an_unreachable_engine_blocks_with_its_code() {
        let mut f = up(Value::Null, Value::Null);
        f.engine = Engine::Unreachable("connection refused".into());
        let r = evaluate(&f);
        assert_eq!(r["status"], "blocked");
        assert_eq!(
            status(&r, "engine"),
            ("failed".into(), Some("DOCS_ENGINE_UNAVAILABLE".into()))
        );
    }

    #[test]
    fn permissions_fail_with_the_doctor_codes() {
        let mut f = up(health(), json!({"items": []}));
        f.state_dir = Some(DirFacts {
            mode: 0o40755,
            ..dir()
        });
        f.files = vec![
            FileFacts {
                mode: 0o100644,
                ..token("serve.token")
            },
            FileFacts {
                nlink: 2,
                ..token("engine.token")
            },
        ];
        let r = evaluate(&f);
        assert_eq!(
            status(&r, "state_dir"),
            ("failed".into(), Some("DOCS_DOCTOR_STATE_PERMS".into()))
        );
        assert_eq!(
            status(&r, "token_file"),
            ("failed".into(), Some("DOCS_DOCTOR_TOKEN_PERMS".into()))
        );
        let mut f = up(health(), json!({"items": []}));
        f.state_dir = Some(DirFacts {
            symlink: true,
            ..dir()
        });
        assert_eq!(
            status(&evaluate(&f), "state_dir").1.as_deref(),
            Some("DOCS_DOCTOR_STATE_PERMS")
        );
        let mut f = up(health(), json!({"items": []}));
        f.state_dir = Some(DirFacts {
            owned_by_me: false,
            ..dir()
        });
        assert_eq!(status(&evaluate(&f), "state_dir").0, "failed");
    }

    #[test]
    fn engine_side_problems_have_their_codes() {
        let mut h = health();
        h["disk"]["free_bytes"] = json!(1_000_000u64);
        h["disk"]["cas_sample"] = json!({"checked": 20, "corrupt": 1});
        h["queue"]["stuck"] = json!(1);
        h["components"]["parser"]["status"] = json!("unavailable");
        h["schema_versions"] = json!([]);
        let r = evaluate(&up(h, json!({"items": []})));
        assert_eq!(r["status"], "blocked");
        assert_eq!(
            status(&r, "disk").1.as_deref(),
            Some("DOCS_ENGINE_DISK_LOW")
        );
        assert_eq!(
            status(&r, "cas_sample").1.as_deref(),
            Some("DOCS_ENGINE_CAS_CORRUPT")
        );
        assert_eq!(
            status(&r, "stuck_jobs").1.as_deref(),
            Some("DOCS_DOCTOR_STUCK_JOBS")
        );
        assert_eq!(
            status(&r, "parser").1.as_deref(),
            Some("DOCS_DOCTOR_PARSER")
        );
        assert_eq!(
            status(&r, "schema").1.as_deref(),
            Some("DOCS_ENGINE_SCHEMA_MISMATCH")
        );
        // quarantine is information only.
        assert_eq!(status(&r, "quarantine").0, "ok");
    }

    #[test]
    fn a_database_that_is_down_is_the_engine_failure() {
        let mut h = health();
        h["status"] = json!("unavailable");
        h["components"]["db"] = json!({"status": "unavailable"});
        let r = evaluate(&up(h, json!({"items": []})));
        assert_eq!(
            status(&r, "engine").1.as_deref(),
            Some("DOCS_ENGINE_DB_UNAVAILABLE")
        );
    }

    #[test]
    fn drives_report_the_degraded_reason_and_unreadable_roots() {
        let drives = json!({"items": [
            {"id": "d1", "freshness": {"state": "degraded", "reason_code": "DOCS_SOURCE_ROOT_MISSING"}},
            {"id": "d2", "freshness": {"state": "up_to_date"}},
        ]});
        let r = evaluate(&up(health(), drives));
        assert_eq!(
            status(&r, "drives"),
            ("failed".into(), Some("DOCS_SOURCE_ROOT_MISSING".into()))
        );
        let mut f = up(
            health(),
            json!({"items": [{"id": "d2", "freshness": {"state": "up_to_date"}}]}),
        );
        f.roots = vec![Root {
            drive_id: "d2".into(),
            readable: false,
        }];
        assert_eq!(status(&evaluate(&f), "drives").0, "failed");
    }

    #[test]
    fn container_mode_is_not_covered_yet() {
        let mut f = up(health(), json!({"items": []}));
        f.engine_mode = Some("compose".into());
        let r = evaluate(&f);
        assert_eq!(status(&r, "docker").0, "not_covered");
        assert_eq!(r["status"], "ok");
    }

    #[test]
    fn the_report_never_carries_a_token_or_path() {
        let r = evaluate(&up(health(), json!({"items": []}))).to_string();
        assert!(!r.contains("oodi_") && !r.contains("/Users"));
    }

    // ---- gather: a real state directory and a stand-in engine over loopback

    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;

    /// Answers `/v1/docs/health` and `/v1/docs/drives` for `n` requests.
    fn stand_in(health: Value, drives: Value, n: usize) -> (u16, std::thread::JoinHandle<()>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            for _ in 0..n {
                let Ok((mut s, _)) = l.accept() else { return };
                let mut buf = [0u8; 4096];
                let got = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..got]).to_lowercase();
                assert!(
                    req.contains("authorization: bearer "),
                    "the token is sent: {req}"
                );
                let body = if req.starts_with("get /v1/docs/health") {
                    &health
                } else {
                    &drives
                };
                let text = body.to_string();
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        (port, h)
    }

    fn state_dir(name: &str, port: Option<u16>) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oo-doctor-docs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("engine")).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        if let Some(port) = port {
            let token = d.join("serve.token");
            std::fs::write(&token, "oodc_test_token_0123456789abcdef").unwrap();
            std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::fs::write(
                d.join("serve.json"),
                json!({"protocol": "oo.docs.serve/v1", "pid": 1, "port": port,
                       "token_file": token, "engine": {"mode": "host_process"}})
                .to_string(),
            )
            .unwrap();
        }
        d
    }

    fn run(f: impl std::future::Future<Output = Value>) -> Value {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn a_running_engine_is_read_over_loopback_with_the_token_and_passes() {
        let (port, engine) = stand_in(
            health(),
            json!({"items": [{"id": "d1", "backend": "fs", "config": {"root": "/tmp"}, "freshness": {"state": "up_to_date"}}]}),
            2,
        );
        let dir = state_dir("up", Some(port));
        let r = run(async { evaluate(&gather(&dir).await) });
        engine.join().unwrap();
        assert_eq!(r["status"], "ok", "{r}");
        assert_eq!(status(&r, "engine").0, "ok");
        assert_eq!(
            status(&r, "drives").0,
            "ok",
            "the local root /tmp is readable"
        );
        assert_eq!(status(&r, "token_file").0, "ok");
        assert!(
            !r.to_string().contains("oodc_test_token"),
            "no token in the report"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_state_directory_with_loose_permissions_blocks_and_a_missing_one_does_not() {
        let dir = state_dir("perms", None);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let r = run(async { evaluate(&gather(&dir).await) });
        assert_eq!(r["status"], "blocked");
        assert_eq!(
            status(&r, "state_dir"),
            ("failed".into(), Some("DOCS_DOCTOR_STATE_PERMS".into()))
        );
        assert_eq!(status(&r, "engine").0, "not_running");
        let _ = std::fs::remove_dir_all(&dir);
        let none = std::env::temp_dir().join(format!("oo-doctor-docs-none-{}", std::process::id()));
        let r = run(async { evaluate(&gather(&none).await) });
        assert_eq!(r["status"], "not_configured");
    }

    #[test]
    fn a_serve_json_without_an_answering_engine_is_the_engine_failure() {
        // A port nobody listens on.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let dir = state_dir("down", Some(port));
        let r = run(async { evaluate(&gather(&dir).await) });
        assert_eq!(
            status(&r, "engine"),
            ("failed".into(), Some("DOCS_ENGINE_UNAVAILABLE".into()))
        );
        assert_eq!(r["status"], "blocked");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
