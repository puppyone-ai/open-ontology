#[cfg(test)]
mod a164_rollback_depth {
    //! Review-only. Observes driver depth around `PgSession::rollback`.
    //! Production sources are not changed above this module.

    use std::fs;
    use std::time::Duration;

    use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode, PgTransactionManager};
    use sqlx::{ConnectOptions, Row};
    use sqlx_core::transaction::TransactionManager;

    use super::PgSession;
    use crate::sql;
    use crate::{
        PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef,
        SelectedTable, SourceId, TableSelection,
    };

    #[tokio::test]
    #[ignore = "one private PostgreSQL 17.10; OO_IMPORT_PG_HOST must be set by the deciding run"]
    async fn sequential_begins_keep_depth_and_poison_stays_until_drop() {
        let mut admin = admin_conn().await;
        setup(&mut admin).await;
        let result = exercise(&mut admin).await;
        cleanup(&mut admin).await;
        result.expect("rollback depth");
    }

    async fn exercise(admin: &mut PgConnection) -> Result<(), String> {
        let mut lines = Vec::new();
        let mut session = reader_session().await?;
        let report = session
            .inspect_access()
            .await
            .map_err(|err| format!("inspect {}", err.fact().unwrap_or("none")))?;
        if !report.registration_allowed {
            return Err("reader registration was refused".to_string());
        }
        let depth = PgTransactionManager::get_transaction_depth(&session.conn);
        if depth != 0 || session.in_transaction || session.poisoned {
            return Err(format!(
                "after inspect depth={depth} in_tx={} poisoned={}",
                session.in_transaction, session.poisoned
            ));
        }
        let catalog = session
            .catalog()
            .await
            .map_err(|err| format!("catalog {}", err.fact().unwrap_or("none")))?;
        let depth = PgTransactionManager::get_transaction_depth(&session.conn);
        if depth != 0 || session.in_transaction || session.poisoned {
            return Err(format!(
                "after catalog depth={depth} in_tx={} poisoned={}",
                session.in_transaction, session.poisoned
            ));
        }
        if catalog.tables.len() != 1 || catalog.tables[0].name != "ribs" {
            return Err(format!("catalog tables {:?}", catalog.tables.iter().map(|table| &table.name).collect::<Vec<_>>()));
        }
        session.begin().await.map_err(|err| format!("second begin {}", err.fact().unwrap_or("none")))?;
        if PgTransactionManager::get_transaction_depth(&session.conn) != 1 || !session.in_transaction {
            return Err("explicit begin did not reach depth 1".to_string());
        }
        session.rollback().await.map_err(|err| format!("rollback {}", err.fact().unwrap_or("none")))?;
        if PgTransactionManager::get_transaction_depth(&session.conn) != 0
            || session.in_transaction
            || session.poisoned
        {
            return Err("depth-1 rollback did not clear the driver".to_string());
        }
        session.begin().await.map_err(|err| format!("begin after rollback {}", err.fact().unwrap_or("none")))?;
        if PgTransactionManager::get_transaction_depth(&session.conn) != 1 {
            return Err("begin after rollback did not reach depth 1".to_string());
        }
        let statement = sqlx_core::transaction::rollback_ansi_transaction_sql(1);
        if statement.as_str() != sql::ROLLBACK {
            return Err("depth 1 rollback text is not ROLLBACK".to_string());
        }
        session.rollback().await.map_err(|err| format!("second rollback {}", err.fact().unwrap_or("none")))?;
        lines.push("sequential_inspect_catalog=depth_0".to_string());
        lines.push("explicit_begin_rollback_begin=depth_1_0_1".to_string());
        drop(session);

        let mut flagged = reader_session().await?;
        flagged.in_transaction = true;
        let err = flagged.rollback().await.expect_err("depth 0 must poison");
        if err.fact() != Some("rollback")
            || !flagged.poisoned
            || flagged.in_transaction
            || PgTransactionManager::get_transaction_depth(&flagged.conn) != 0
        {
            return Err(format!("depth0 poison fact={}", err.fact().unwrap_or("none")));
        }
        let closed = flagged.ensure_open().expect_err("poisoned session");
        if closed.fact() != Some("session_poisoned") {
            return Err(format!("ensure_open {}", closed.fact().unwrap_or("none")));
        }
        let idle = state_for_app(admin).await;
        if idle != "0" {
            return Err(format!("depth0 poison left idle-in-transaction count {idle}"));
        }
        drop(flagged);
        lines.push("depth0_poison=rollback_no_sql".to_string());

        let mut nested = reader_session().await?;
        nested.begin().await.map_err(|err| format!("nest begin {}", err.fact().unwrap_or("none")))?;
        PgTransactionManager::begin(&mut nested.conn, None)
            .await
            .map_err(|_| "savepoint begin failed".to_string())?;
        if PgTransactionManager::get_transaction_depth(&nested.conn) != 2 {
            return Err("nested begin did not reach depth 2".to_string());
        }
        let pid: i32 = sqlx::query("SELECT pg_catalog.pg_backend_pid()")
            .fetch_one(&mut nested.conn)
            .await
            .map_err(|_| "pid query failed".to_string())?
            .try_get(0)
            .map_err(|_| "pid decode failed".to_string())?;
        let err = nested.rollback().await.expect_err("depth 2 must poison");
        if err.fact() != Some("rollback")
            || !nested.poisoned
            || nested.in_transaction
            || PgTransactionManager::get_transaction_depth(&nested.conn) != 2
        {
            return Err(format!("depth2 poison fact={}", err.fact().unwrap_or("none")));
        }
        let state = backend_state(admin, pid).await?;
        if state != "idle in transaction" {
            return Err(format!("depth2 poison state={state}"));
        }
        drop(nested);
        wait_pid_gone(admin, pid).await?;
        lines.push(format!("depth2_poison=no_rollback_sql pid_closed={pid}"));
        lines.push("cleanup=socket_drop".to_string());
        write_outcome(&lines)?;
        Ok(())
    }

    fn write_outcome(lines: &[String]) -> Result<(), String> {
        let path = std::env::var("OO_IMPORT_PG_EVIDENCE").map_err(|_| "evidence dir missing".to_string())?;
        fs::create_dir_all(&path).map_err(|_| "evidence dir".to_string())?;
        fs::write(format!("{path}/depth-outcome.txt"), lines.join("\n") + "\n").map_err(|_| "outcome write".to_string())
    }

    async fn reader_session() -> Result<PgSession, String> {
        let host = std::env::var("OO_IMPORT_PG_HOST").map_err(|_| "host".to_string())?;
        let port: u16 = std::env::var("OO_IMPORT_PG_PORT")
            .map_err(|_| "port".to_string())?
            .parse()
            .map_err(|_| "port".to_string())?;
        let password = fs::read_to_string(env_path("OO_IMPORT_PG_READER_PASSWORD_FILE")?)
            .map_err(|_| "reader password".to_string())?;
        let ca = fs::read(env_path("OO_IMPORT_PG_CA_FILE")?).map_err(|_| "ca".to_string())?;
        let config = PgSourceConfig::try_new(
            SourceId::new("a164depth").map_err(|_| "source id".to_string())?,
            PgEndpoint::new(&host, port, "oo_a164_depth").map_err(|_| "endpoint".to_string())?,
            PgCredentials::new("oo_a164_depth_reader", password.trim()).map_err(|_| "credentials".to_string())?,
            SecretRef::new("secret://pg/a164/depth").map_err(|_| "secret ref".to_string())?,
            PgTlsConfig::from_ca_pem(ca).map_err(|_| "tls".to_string())?,
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(30),
                Duration::from_secs(15),
                10_000,
                8_388_608,
            )
            .map_err(|_| "budget".to_string())?,
            TableSelection::new(vec![SelectedTable::new("yard", "ribs").map_err(|_| "table".to_string())?])
                .map_err(|_| "selection".to_string())?,
        )
        .map_err(|_| "config".to_string())?;
        PgConnector::new(config)
            .connect()
            .await
            .map_err(|err| format!("connect {}", err.fact().unwrap_or("none")))
    }

    async fn admin_conn() -> PgConnection {
        let host = std::env::var("OO_IMPORT_PG_HOST").expect("host");
        let port: u16 = std::env::var("OO_IMPORT_PG_PORT").expect("port").parse().expect("port");
        let user = std::env::var("OO_IMPORT_PG_ADMIN_USER").expect("admin user");
        let password = fs::read_to_string(env_path("OO_IMPORT_PG_ADMIN_PASSWORD_FILE").expect("admin file"))
            .expect("admin password");
        let ca = fs::read(env_path("OO_IMPORT_PG_CA_FILE").expect("ca")).expect("ca");
        let options = PgConnectOptions::from_explicit()
            .host(&host)
            .port(port)
            .username(&user)
            .password(password.trim())
            .database("postgres")
            .ssl_mode(PgSslMode::VerifyFull)
            .ssl_root_cert_from_pem(ca);
        assert!(options.get_options().is_none());
        options.disable_statement_logging().connect().await.expect("admin dial")
    }

    fn env_path(key: &str) -> Result<String, String> {
        std::env::var(key).map_err(|_| key.to_string())
    }

    async fn setup(admin: &mut PgConnection) {
        cleanup(admin).await;
        let password = fs::read_to_string(env_path("OO_IMPORT_PG_READER_PASSWORD_FILE").expect("reader"))
            .expect("reader password");
        let password = password.trim();
        assert!(password.chars().all(|ch| ch.is_ascii_hexdigit()));
        exec(
            admin,
            &format!(
                "CREATE ROLE oo_a164_depth_reader LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"
            ),
        )
        .await;
        exec(
            admin,
            "CREATE DATABASE oo_a164_depth TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'",
        )
        .await;
        let mut source = dial_db(admin, "oo_a164_depth").await;
        exec(&mut source, "CREATE SCHEMA yard").await;
        exec(
            &mut source,
            "CREATE TABLE yard.ribs (id int4 PRIMARY KEY, wood text NOT NULL)",
        )
        .await;
        exec(&mut source, "INSERT INTO yard.ribs (id, wood) VALUES (1, 'oak')").await;
        exec(&mut source, "GRANT USAGE ON SCHEMA yard TO oo_a164_depth_reader").await;
        exec(&mut source, "GRANT SELECT ON yard.ribs TO oo_a164_depth_reader").await;
    }

    async fn cleanup(admin: &mut PgConnection) {
        exec(admin, "DROP DATABASE IF EXISTS oo_a164_depth WITH (FORCE)").await;
        exec(admin, "DROP ROLE IF EXISTS oo_a164_depth_reader").await;
    }

    async fn dial_db(admin: &PgConnection, database: &str) -> PgConnection {
        let host = std::env::var("OO_IMPORT_PG_HOST").expect("host");
        let port: u16 = std::env::var("OO_IMPORT_PG_PORT").expect("port").parse().expect("port");
        let user = std::env::var("OO_IMPORT_PG_ADMIN_USER").expect("admin user");
        let password = fs::read_to_string(env_path("OO_IMPORT_PG_ADMIN_PASSWORD_FILE").expect("admin file"))
            .expect("admin password");
        let ca = fs::read(env_path("OO_IMPORT_PG_CA_FILE").expect("ca")).expect("ca");
        let _ = admin;
        let options = PgConnectOptions::from_explicit()
            .host(&host)
            .port(port)
            .username(&user)
            .password(password.trim())
            .database(database)
            .ssl_mode(PgSslMode::VerifyFull)
            .ssl_root_cert_from_pem(ca);
        options.disable_statement_logging().connect().await.expect("source dial")
    }

    async fn exec(conn: &mut PgConnection, sql: &str) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
            .execute(&mut *conn)
            .await
            .expect("fixture sql");
    }

    async fn state_for_app(admin: &mut PgConnection) -> String {
        scalar(
            admin,
            "SELECT count(*)::text FROM pg_catalog.pg_stat_activity WHERE application_name = 'oo-pg-source' AND state = 'idle in transaction' AND datname = 'oo_a164_depth'",
        )
        .await
    }

    async fn backend_state(admin: &mut PgConnection, pid: i32) -> Result<String, String> {
        let row = sqlx::query("SELECT state FROM pg_catalog.pg_stat_activity WHERE pid = $1")
            .bind(pid)
            .fetch_optional(&mut *admin)
            .await
            .map_err(|_| "state query failed".to_string())?;
        match row {
            Some(row) => row.try_get(0).map_err(|_| "state decode".to_string()),
            None => Err("backend missing before drop".to_string()),
        }
    }

    async fn wait_pid_gone(admin: &mut PgConnection, pid: i32) -> Result<(), String> {
        for _ in 0..30 {
            let n = scalar(admin, &format!("SELECT count(*)::text FROM pg_catalog.pg_stat_activity WHERE pid = {pid}")).await;
            if n == "0" {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(format!("backend {pid} remained after drop"))
    }

    async fn scalar(conn: &mut PgConnection, sql: &str) -> String {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
            .fetch_one(&mut *conn)
            .await
            .expect("scalar")
    }
}
