// Baseline reproduction appended only to the pre-fix archive copy.
// A stalled cancel TCP is outside the 750 ms poll, so Drop is still blocked
// at 1500 ms and a pending CancelHandle can lend the next generation.

async fn prove_baseline_stall(env: &mut Env) {
    let upstream = "oo-a189-pg:5432";
    let proxy = StallProxy::start(15432, upstream.to_string())
        .await
        .expect("red proxy");
    ensure_proxy_name("oo-a196-proxy").expect("proxy name");
    prove_baseline_drop(env, &proxy, upstream).await;
    prove_baseline_race(env, &proxy, upstream).await;
    proxy.task.abort();
}

async fn prove_baseline_drop(env: &mut Env, proxy: &StallProxy, upstream: &str) {
    proxy.stall.store(false, std::sync::atomic::Ordering::SeqCst);
    proxy.release(upstream).await;
    let hold = hold_catalog(env);
    let identity = identity_for(ROLE_A);
    let mut session = open_session_at(env, "oo-a196-proxy", 15432).await;
    let pid = session.backend_pid();
    let stream = session
        .execute_trusted_select(
            req(
                &identity,
                &hold,
                "SELECT fixture.page_hold() FROM logistics.shipment LIMIT 1",
                ROLE_A,
                10,
                BYTES_OK,
                30_000,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("red drop stream");
    assert!(
        wait_user_query(&env.admin, pid, &env.shadow.table).await,
        "red drop query did not become active"
    );
    let before = proxy.accepts.load(std::sync::atomic::Ordering::SeqCst);
    proxy.stall.store(true, std::sync::atomic::Ordering::SeqCst);
    let started = Instant::now();
    drop(stream);
    let idle = session.wait_idle();
    tokio::pin!(idle);
    let blocked = tokio::select! {
        result = idle.as_mut() => {
            result.expect("red drop idle");
            false
        }
        () = tokio::time::sleep(Duration::from_millis(1_500)) => true,
    };
    let stalled_ms = started.elapsed().as_millis();
    let view = observe_backend(&env.admin, pid)
        .await
        .expect("red drop observe");
    let active = view.as_ref().is_some_and(|row| row.state() == "active");
    let accepts = proxy.accepts.load(std::sync::atomic::Ordering::SeqCst).saturating_sub(before);
    env.oracle.fact(
        "red_drop",
        &format!("blocked:{blocked} ms:{stalled_ms} active:{active} accepts:{accepts}"),
    );
    assert!(blocked, "baseline drop returned before 1500 ms");
    assert!(stalled_ms >= 1_500, "baseline drop elapsed {stalled_ms}");
    assert!(active, "baseline backend was not still active");
    assert!(accepts >= 1, "baseline cancel transport was not accepted");
    proxy.stall.store(false, std::sync::atomic::Ordering::SeqCst);
    proxy.release(upstream).await;
    idle.await.expect("red drop finished after release");
}

async fn prove_baseline_race(env: &mut Env, proxy: &StallProxy, upstream: &str) {
    proxy.stall.store(false, std::sync::atomic::Ordering::SeqCst);
    proxy.release(upstream).await;
    let brief_oid = install_brief_hold(&env.admin).await;
    let mut catalog = hold_catalog(env);
    catalog.functions.push(ResolvedFunction {
        oid: brief_oid,
        schema: "fixture".to_string(),
        name: "a196_brief".to_string(),
        argument_type_oids: Vec::new(),
        aggregate_star: false,
    });
    let identity = identity_for(ROLE_A);
    let mut session = open_session_at(env, "oo-a196-proxy", 15432).await;
    let pid = session.backend_pid();
    let mut brief = session
        .execute_trusted_select(
            req(
                &identity,
                &catalog,
                "SELECT fixture.a196_brief() FROM logistics.shipment LIMIT 1",
                ROLE_A,
                10,
                BYTES_OK,
                30_000,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("red brief stream");
    assert!(
        wait_user_query(&env.admin, pid, &env.shadow.table).await,
        "red brief query did not become active"
    );
    let handle = brief.cancel_handle();
    let direct = connect_db(DATABASE, ADMIN, &env.admin_pass, &env.ca).await;
    let before = proxy.accepts.load(std::sync::atomic::Ordering::SeqCst);
    proxy.stall.store(true, std::sync::atomic::Ordering::SeqCst);
    let cancel_task = tokio::spawn(async move { handle.cancel(&direct).await });
    let held = wait_proxy_accepts(proxy, before, Duration::from_secs(2)).await;
    let row = brief.next_row().await.expect("red brief row");
    assert!(row.is_some(), "red brief query did not finish while cancel was held");
    let end = brief.next_row().await.expect("red brief end");
    assert!(end.is_none(), "red brief did not finish cleanly");
    tokio::time::timeout(Duration::from_secs(5), session.wait_idle())
        .await
        .expect("red brief idle timed out")
        .expect("red brief idle");
    drop(brief);
    let hold = hold_catalog(env);
    let next = session
        .execute_trusted_select(
            req(
                &identity,
                &hold,
                "SELECT fixture.page_hold() FROM logistics.shipment LIMIT 1",
                ROLE_A,
                10,
                BYTES_OK,
                30_000,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    let started_next = next.is_ok();
    let next_active = if let Ok(stream) = next {
        let visible = wait_user_query(&env.admin, pid, &env.shadow.table).await;
        proxy.stall.store(false, std::sync::atomic::Ordering::SeqCst);
        proxy.release(upstream).await;
        drop(stream);
        session.wait_idle().await.expect("red next idle");
        visible
    } else {
        proxy.stall.store(false, std::sync::atomic::Ordering::SeqCst);
        proxy.release(upstream).await;
        false
    };
    let cancel_label = match tokio::time::timeout(Duration::from_secs(3), cancel_task).await {
        Ok(Ok(Ok(CancelOutcome::Sent))) => "sent",
        Ok(Ok(Ok(CancelOutcome::NotSent))) => "not_sent",
        Ok(Ok(Err(_))) => "error",
        Ok(Err(_)) => "join",
        Err(_) => "timeout",
    };
    env.oracle.fact(
        "red_race",
        &format!(
            "held:{held} next_started:{started_next} next_active:{next_active} cancel:{cancel_label}"
        ),
    );
    assert!(held, "red cancel transport was not held");
    assert!(
        started_next && next_active,
        "baseline lent the next generation while cancel transport was pending"
    );
}

async fn install_brief_hold(client: &Client) -> u32 {
    client
        .batch_execute(
            "CREATE FUNCTION fixture.a196_brief() RETURNS integer\n\
             LANGUAGE plpgsql VOLATILE AS $$\n\
             BEGIN PERFORM pg_catalog.pg_sleep(0.4); RETURN 1; END $$;\n\
             REVOKE ALL ON FUNCTION fixture.a196_brief() FROM PUBLIC;\n\
             GRANT EXECUTE ON FUNCTION fixture.a196_brief() TO oo_r_reader_a",
        )
        .await
        .expect("brief hold");
    let oid: i32 = client
        .query_one(
            "SELECT p.oid::int4 FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'fixture' AND p.proname = 'a196_brief' AND p.pronargs = 0",
            &[],
        )
        .await
        .expect("brief oid")
        .get(0);
    u32::try_from(oid).expect("brief oid range")
}

async fn open_session_at(env: &Env, host: &str, port: u16) -> DedicatedSession {
    let endpoint = TlsEndpoint {
        host: host.to_string(),
        port,
        database: DATABASE.to_string(),
        user: LOGIN.to_string(),
    };
    let (client, connection) = connect_require_tls(&endpoint, &env.login_pass, &env.ca)
        .await
        .expect("session connect");
    let tls = rustls_connector(&env.ca).expect("connector");
    DedicatedSession::bind(client, connection, tls)
        .await
        .expect("bind")
}

fn ensure_proxy_name(name: &str) -> Result<(), String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|_| "local-ip".to_string())?;
    socket
        .connect("oo-a189-pg:5432")
        .map_err(|_| "local-route".to_string())?;
    let ip = socket.local_addr().map_err(|_| "local-addr".to_string())?;
    let mut hosts = fs::read_to_string("/etc/hosts").map_err(|_| "hosts-read".to_string())?;
    if !hosts.contains(name) {
        if !hosts.ends_with('\n') {
            hosts.push('\n');
        }
        hosts.push_str(&format!("{} {name}\n", ip.ip()));
        fs::write("/etc/hosts", hosts).map_err(|_| "hosts-write".to_string())?;
    }
    Ok(())
}

async fn wait_proxy_accepts(proxy: &StallProxy, before: usize, bound: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < bound {
        if proxy.accepts.load(std::sync::atomic::Ordering::SeqCst) > before {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

struct StallProxy {
    stall: std::sync::Arc<std::sync::atomic::AtomicBool>,
    held: std::sync::Arc<tokio::sync::Mutex<Vec<tokio::net::TcpStream>>>,
    accepts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl StallProxy {
    async fn start(port: u16, upstream: String) -> Result<Self, String> {
        let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
            .await
            .map_err(|_| "proxy-bind".to_string())?;
        let stall = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let held = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let accepts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let stall_task = std::sync::Arc::clone(&stall);
        let held_task = std::sync::Arc::clone(&held);
        let accepts_task = std::sync::Arc::clone(&accepts);
        let task = tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                if stall_task.load(std::sync::atomic::Ordering::SeqCst) {
                    accepts_task.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    held_task.lock().await.push(sock);
                } else {
                    let upstream = upstream.clone();
                    tokio::spawn(async move {
                        let _ = bridge_proxy(sock, &upstream).await;
                    });
                }
            }
        });
        Ok(Self {
            stall,
            held,
            accepts,
            task,
        })
    }

    async fn release(&self, upstream: &str) {
        self.stall.store(false, std::sync::atomic::Ordering::SeqCst);
        let drained: Vec<tokio::net::TcpStream> = self.held.lock().await.drain(..).collect();
        for sock in drained {
            let upstream = upstream.to_string();
            tokio::spawn(async move {
                let _ = bridge_proxy(sock, &upstream).await;
            });
        }
    }
}

async fn bridge_proxy(mut inbound: tokio::net::TcpStream, upstream: &str) -> std::io::Result<()> {
    let mut outbound = tokio::net::TcpStream::connect(upstream).await?;
    tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await?;
    Ok(())
}
