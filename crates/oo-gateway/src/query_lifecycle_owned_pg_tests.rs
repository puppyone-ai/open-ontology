//! Real sockets, native CLI and an independent SQL oracle on the owned fixture.
use super::*;
use tokio::io::AsyncWriteExt;

async fn cli_call(f: &Fixture, origin: &str, args: Vec<String>) -> (i32, Value) {
    let binary = std::env::var("OO_F22_CLI").unwrap();
    let token = f.root.join("lifecycle.token");
    fs::write(&token, &f.token).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut args = args;
    args.extend([
        "--json".into(),
        "--gateway".into(),
        origin.into(),
        "--transport".into(),
        "local-development".into(),
        "--auth".into(),
        "external".into(),
        "--token-file".into(),
        token.to_str().unwrap().into(),
    ]);
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(binary)
            .args(args)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    fs::write(f.root.join(format!("cli-{stamp}.stdout")), &output.stdout).unwrap();
    fs::write(f.root.join(format!("cli-{stamp}.stderr")), &output.stderr).unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).expect("CLI JSON");
    assert!(!value.to_string().contains(&f.token));
    (output.status.code().unwrap(), value)
}

async fn request(f: &Fixture, sql: &str, timeout_ms: u64) -> (u16, Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/v1/query")
        .header("authorization", format!("Bearer {}", f.token))
        .header("content-type", "application/json")
        .header(EXPECTED, "current")
        .body(Body::from(
            json!({"sql":sql,"format":"json","timeout_ms":timeout_ms}).to_string(),
        ))
        .unwrap();
    let response = crate::http_gateway::router(f.state.clone())
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn running(f: &Fixture) -> String {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let flights = f.plane.inflight.lock().await;
            if let Some(flight) = flights.values().find(|f| f.state.status() == "running") {
                return flight.query_id.clone();
            }
            drop(flights);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("registered before metadata")
}

async fn backend_idle(f: &Fixture) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let active: i64 = f
                .admin
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity WHERE usename='oo_f22_login' \
                 AND (state='active' OR state LIKE 'idle in transaction%')",
                    &[],
                )
                .await
                .unwrap()
                .get(0);
            if active == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("abandoned database work cleaned up");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "new isolated PG only: tests/f22_owned_pg.py <evidence> --lifecycle"]
async fn query_lifecycle_owned_pg() {
    let f = Fixture::with_dates(true).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = format!("http://{address}");
    let router = crate::http_gateway::router(f.state.clone());
    let server = BindingDriver(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let date_column = format!("pc_{}", "c".repeat(60));
    let mut receipts = Vec::new();
    let mut bound_id = String::new();
    for day in 22..=28 {
        let sql = format!(
            "SELECT COUNT(*) AS n, SUM(amount) AS total FROM app.items \
            WHERE event_time >= TIMESTAMP '2026-10-{day} 00:00:00' AND event_time < TIMESTAMP '2026-10-29 00:00:00'"
        );
        let gold = f.admin.query_one(&format!("SELECT count(*)::text,sum({})::text FROM _oo.{} \
            WHERE {date_column} >= TIMESTAMP '2026-10-{day}' AND {date_column} < TIMESTAMP '2026-10-29'",
            f.column, f.physical), &[]).await.unwrap();
        let expected = json!([[gold.get::<_, String>(0), gold.get::<_, String>(1)]]);
        let started = Instant::now();
        let (exit, result) = cli_call(
            &f,
            &origin,
            vec![
                "query".into(),
                "--expected-publication-id".into(),
                "current".into(),
                sql,
            ],
        )
        .await;
        assert_eq!(exit, 0, "{result}");
        assert_eq!(result["rows"], expected);
        assert_eq!(result["truncated"], false);
        bound_id = result["publication_binding"]["publication_id"]
            .as_str()
            .unwrap()
            .into();
        receipts.push(json!({"start_day":day,"expected":expected,"result":result,"elapsed_ms":started.elapsed().as_millis()}));
    }
    let followup = "SELECT COUNT(*) AS n, SUM(amount) AS total FROM app.items";
    // Whole-request budget includes a blocked catalog, not just the final portal.
    f.admin
        .batch_execute("BEGIN; LOCK TABLE _oo.physical_mappings IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let start = Instant::now();
    let (exit, failure) = cli_call(
        &f,
        &origin,
        vec![
            "query".into(),
            "--timeout-ms".into(),
            "180".into(),
            followup.into(),
        ],
    )
    .await;
    assert_eq!(exit, 2, "{failure}");
    assert_eq!(failure["class"], "gateway");
    assert_eq!(failure["code"], "TIMEOUT");
    assert!(start.elapsed() < Duration::from_secs(2));
    let qid = failure["query_id"].as_str().unwrap();
    let (_, status) = cli_call(&f, &origin, vec!["status".into(), qid.into()]).await;
    assert_eq!(status["status"], "timed_out");
    backend_idle(&f).await;
    f.admin.batch_execute("ROLLBACK").await.unwrap();
    assert_eq!(request(&f, followup, 3000).await.0, 200);
    receipts.push(json!({"catalog_timeout":failure,"status":status}));

    // A blocked role is rejected quickly instead of holding another execution slot.
    f.admin
        .batch_execute(&format!(
            "BEGIN; LOCK TABLE _oo.{} IN ACCESS EXCLUSIVE MODE",
            f.physical
        ))
        .await
        .unwrap();
    let blocked = request(&f, followup, 4500);
    tokio::pin!(blocked);
    let qid = tokio::select! {
        qid = running(&f) => qid,
        result = &mut blocked => panic!("expected blocked request: {result:?}"),
    };
    let parallel = async {
        let start = Instant::now();
        let (code, busy) = request(&f, followup, 1000).await;
        assert_eq!(code, 503, "{busy}");
        assert_eq!(busy["error"]["code"], "OVERLOADED");
        assert!(start.elapsed() < Duration::from_secs(1));
        let (_, operations) = cli_call(&f, &origin, vec!["operations".into()]).await;
        assert!(
            operations["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["query_id"] == qid)
        );
        let (_, cancellation) = cli_call(&f, &origin, vec!["cancel".into(), qid.clone()]).await;
        assert_eq!(cancellation["outcome"], "requested");
        cancellation
    };
    let (result, cancellation) = tokio::join!(&mut blocked, parallel);
    assert_eq!(result.1["error"]["code"], "TIMEOUT", "{result:?}");
    assert_eq!(result.1["error"]["message"], "query canceled");
    backend_idle(&f).await;
    f.admin.batch_execute("ROLLBACK").await.unwrap();
    let (_, status) = cli_call(&f, &origin, vec!["status".into(), qid.clone()]).await;
    assert_eq!(status["status"], "canceled");
    let (_, second_cancel) = cli_call(&f, &origin, vec!["cancel".into(), qid]).await;
    assert_eq!(second_cancel["outcome"], "not_running");
    assert_eq!(request(&f, followup, 3000).await.0, 200);
    receipts.push(json!({"cancel":cancellation,"status":status,"repeat_cancel":second_cancel}));

    // Drop a real HTTP connection while a statement is blocked.
    f.admin
        .batch_execute(&format!(
            "BEGIN; LOCK TABLE _oo.{} IN ACCESS EXCLUSIVE MODE",
            f.physical
        ))
        .await
        .unwrap();
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let body = json!({"sql":followup,"format":"json","timeout_ms":4500}).to_string();
    socket.write_all(format!("POST /v1/query HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",f.token,body.len()).as_bytes()).await.unwrap();
    let qid = running(&f).await;
    drop(socket);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let (_, _, status) = f
                .http(
                    "GET",
                    &format!("/v1/queries/{qid}"),
                    None,
                    None,
                    None,
                    &f.token,
                )
                .await;
            if status["status"] == "abandoned" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("HTTP disconnect retires lifecycle");
    backend_idle(&f).await;
    f.admin.batch_execute("ROLLBACK").await.unwrap();
    assert_eq!(request(&f, followup, 3000).await.0, 200);
    receipts.push(json!({"disconnect_query_id":qid,"status":"abandoned","followup":200}));

    // Same principal in another role gets an independent session, not ownership
    // of the first role's query. Its work is not stuck behind the busy role.
    let mut other_tuple = tuple("alice");
    other_tuple.role = "oo_r_f22_other".into();
    let (other_token, _) = crate::http_gateway::harness_issue(
        Endpoint {
            host: f.endpoint.host.clone(),
            port: f.endpoint.port,
            database: f.endpoint.database.clone(),
        },
        &f.password,
        &f.ca,
        IssueGrant {
            kind: TokenKind::Client,
            client_ttl_ms: None,
            current: other_tuple,
        },
    )
    .await
    .unwrap();
    f.admin.batch_execute("CREATE OR REPLACE FUNCTION public.f22_read_probe() RETURNS boolean LANGUAGE plpgsql VOLATILE AS
        $fn$ BEGIN PERFORM pg_sleep(2); RETURN true; END $fn$").await.unwrap();
    let slow = request(&f, followup, 4500);
    tokio::pin!(slow);
    let qid = tokio::select! {
        qid = running(&f) => qid,
        result = &mut slow => panic!("expected running: {result:?}"),
    };
    let isolated = async {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if f.admin.query_one("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='oo_f22_login' AND wait_event='PgSleep')",
                    &[]).await.unwrap().get::<_,bool>(0) { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        let start = Instant::now();
        let (code, _, other) = f
            .http(
                "POST",
                "/v1/query",
                Some("SELECT COUNT(*) FROM app.items"),
                Some("current"),
                None,
                &other_token,
            )
            .await;
        assert_eq!(code, 200, "{other}");
        assert_eq!(other["rows"], json!([["140000"]]));
        assert!(start.elapsed() < Duration::from_secs(2));
        for (method, suffix) in [("GET", ""), ("POST", "/cancel")] {
            let (code, _, _) = f
                .http(
                    method,
                    &format!("/v1/queries/{qid}{suffix}"),
                    None,
                    None,
                    None,
                    &other_token,
                )
                .await;
            assert_ne!(code, 200, "cross-role lifecycle access");
        }
        assert_eq!(
            f.http(
                "POST",
                &format!("/v1/queries/{qid}/cancel"),
                None,
                None,
                None,
                &f.token
            )
            .await
            .2["outcome"],
            "requested"
        );
        other
    };
    let (slow, other) = tokio::join!(&mut slow, isolated);
    assert_eq!(slow.1["error"]["code"], "TIMEOUT");
    backend_idle(&f).await;
    f.admin.batch_execute("CREATE OR REPLACE FUNCTION public.f22_read_probe() RETURNS boolean LANGUAGE plpgsql VOLATILE AS
        $fn$ BEGIN RETURN true; END $fn$").await.unwrap();
    assert_eq!(request(&f, followup, 3000).await.0, 200);
    receipts.push(json!({"concurrent_other_role":other,"cross_role_status_cancel":"denied","same_role_followup":200}));

    // Real authorization and F22 remain fail-closed.
    for denied in [
        "DELETE FROM app.items",
        "SELECT secret FROM app.items",
        "SELECT pg_sleep(1)",
    ] {
        assert_ne!(request(&f, denied, 3000).await.0, 200, "{denied}");
    }
    f.admin
        .batch_execute(&format!(
            "REVOKE SELECT ({}) ON _oo.{} FROM {ROLE}",
            f.column, f.physical
        ))
        .await
        .unwrap();
    let (code, _, denied) = f
        .http(
            "POST",
            "/v1/query",
            Some(followup),
            Some(&bound_id),
            None,
            &f.token,
        )
        .await;
    assert_ne!(code, 200, "{denied}");
    fs::write(
        f.root.join("query-lifecycle-results.json"),
        serde_json::to_vec_pretty(&json!({
            "scope":"synthetic owned PG, real native CLI and HTTP, never customer/source",
            "rows":140000,"exact_multidate_windows":7,"receipts":receipts,"f22_after_revoke":denied
        }))
        .unwrap(),
    )
    .unwrap();
    drop(server);
}
