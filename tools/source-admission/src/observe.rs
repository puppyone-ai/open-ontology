//! One read-only PostgreSQL session. Statements are fixed SELECTs.
//! The password is not logged. The csvlog, not this function's intent, is
//! the zero-write evidence.

use std::time::Duration;

use sqlx::ConnectOptions;
use sqlx::Row;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use zeroize::Zeroizing;

use crate::audit::{self, AuditReport};
use crate::checklist::scrub_secret;
use crate::model::ObserveConfig;

#[derive(Debug)]
pub struct ObserveError {
    pub fact: String,
}

pub async fn observe(
    config: &ObserveConfig,
    password: &str,
    ca_pem: &[u8],
    log_path: &std::path::Path,
) -> Result<AuditReport, ObserveError> {
    let secret = Zeroizing::new(password.to_string());
    run_selects(config, &secret, ca_pem)
        .await
        .map_err(|err| ObserveError {
            fact: scrub_secret(&err.fact, &secret),
        })?;
    drop(secret);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let log = std::fs::read_to_string(log_path).map_err(|_| ObserveError {
        fact: "log_unreadable".to_string(),
    })?;
    Ok(audit::classify_csv(&log, &config.username))
}

async fn run_selects(
    config: &ObserveConfig,
    password: &str,
    ca_pem: &[u8],
) -> Result<(), ObserveError> {
    if config.host.is_empty() || config.port == 0 || config.username.is_empty() || ca_pem.is_empty()
    {
        return Err(ObserveError {
            fact: "config".to_string(),
        });
    }
    let options = PgConnectOptions::from_explicit()
        .host(&config.host)
        .port(config.port)
        .username(&config.username)
        .password(password)
        .database(&config.database)
        .application_name("oo-source-admission")
        .extra_float_digits(2i8)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca_pem.to_vec());
    if options.get_options().is_some() {
        return Err(ObserveError {
            fact: "startup_options".to_string(),
        });
    }
    let options = options.disable_statement_logging();
    let connect = tokio::time::timeout(Duration::from_secs(30), options.connect()).await;
    let mut conn = match connect {
        Ok(Ok(conn)) => conn,
        Ok(Err(err)) => return Err(fail(password, "connect_failed", err)),
        Err(_) => {
            return Err(ObserveError {
                fact: "connect_timeout".to_string(),
            });
        }
    };
    for sql in SELECTS {
        let query = tokio::time::timeout(
            Duration::from_secs(30),
            sqlx::query(*sql).fetch_one(&mut conn),
        )
        .await;
        match query {
            Ok(Ok(row)) => {
                let _ = row.len();
            }
            Ok(Err(err)) => return Err(fail(password, "query_failed", err)),
            Err(_) => {
                return Err(ObserveError {
                    fact: "query_timeout".to_string(),
                });
            }
        }
    }
    Ok(())
}

fn fail(password: &str, kind: &str, err: impl std::fmt::Display) -> ObserveError {
    let detail = scrub_secret(&err.to_string(), password);
    let detail: String = detail.chars().take(300).collect();
    ObserveError {
        fact: format!("{kind}: {detail}"),
    }
}

const SELECTS: &[&str] = &[
    "SELECT set_config('default_transaction_read_only', 'on', false)",
    "SELECT current_user, session_user, current_database(), pg_is_in_recovery()",
    "SELECT count(*)::int8 FROM app.item",
    "SELECT has_table_privilege(current_user, 'app.item', 'SELECT'), has_table_privilege(current_user, 'app.item', 'INSERT'), has_table_privilege(current_user, 'app.item', 'UPDATE'), has_table_privilege(current_user, 'app.item', 'DELETE'), has_schema_privilege(current_user, 'app', 'CREATE')",
];
