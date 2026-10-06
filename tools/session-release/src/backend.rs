//! Cancel one registered backend and then observe it. `pg_cancel_backend`'s boolean
//! and a dropped socket are not evidence that the backend stopped.

use std::time::{Duration, Instant};

use session_store::{rustls_connector, Endpoint};
use tokio_postgres::config::SslMode;
use tokio_postgres::Client;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendReport {
    pub basis: &'static str,
    pub cancel_invoked: bool,
    pub cancel_returned: Option<bool>,
    pub observed_state: String,
    pub slo_miss: bool,
}

pub fn confirmable(report: &BackendReport) -> bool {
    !report.slo_miss && matches!(report.basis, "observed_idle" | "observed_absent")
}

#[allow(clippy::too_many_arguments)]
pub async fn observe_and_cancel(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    pid: i32,
    application_name: &str,
    query_role: &str,
    database: &str,
    deadline: Instant,
    already_missed: bool,
) -> BackendReport {
    let client = match connect(endpoint, user, password, ca_pem).await {
        Ok(client) => client,
        Err("socket_closed") => return report("socket_closed", false, None, String::new(), already_missed),
        Err("timeout") => return report("pg_unavailable", false, None, String::new(), true),
        Err(_) => return report("pg_unavailable", false, None, String::new(), true),
    };
    if privileged(&client, query_role).await.unwrap_or(true) {
        return report("backend_privileged", false, None, String::new(), already_missed);
    }
    let seen = match activity(&client, pid).await {
        Ok(Some(row)) => row,
        Ok(None) => return report("backend_unknown", false, None, String::new(), already_missed),
        Err("socket_closed") => return report("socket_closed", false, None, String::new(), already_missed),
        Err(_) => return report("backend_unknown", false, None, String::new(), already_missed),
    };
    if !owned_row(&seen, application_name, query_role, database) {
        return report("backend_foreign", false, None, seen.state, already_missed);
    }
    if seen.state == "idle" {
        let basis = if already_missed { "stopped_after_deadline" } else { "observed_idle" };
        return report(basis, false, None, seen.state, already_missed);
    }
    let returned = match cancel_backend(&client, pid).await {
        Ok(value) => value,
        Err("socket_closed") => return report("socket_closed", true, None, seen.state, already_missed),
        Err(_) => return report("backend_unknown", true, None, seen.state, already_missed),
    };
    loop {
        match activity(&client, pid).await {
            Ok(Some(row)) if owned_row(&row, application_name, query_role, database) && row.state == "idle" => {
                let missed = already_missed || Instant::now() > deadline;
                let basis = if missed { "stopped_after_deadline" } else { "observed_idle" };
                return report(basis, true, Some(returned), row.state, missed);
            }
            Ok(Some(row)) if !owned_row(&row, application_name, query_role, database) => {
                return report("backend_foreign", true, Some(returned), row.state, already_missed);
            }
            Ok(Some(_)) if Instant::now() > deadline => {
                return report("still_active", true, Some(returned), seen.state, true);
            }
            Ok(Some(_)) => {}
            Ok(None) => match still_active_name(&client, application_name, query_role, database).await {
                Ok(false) => {
                    let missed = already_missed || Instant::now() > deadline;
                    let basis = if missed { "stopped_after_deadline" } else { "observed_absent" };
                    return report(basis, true, Some(returned), String::new(), missed);
                }
                Ok(true) => return report("still_active", true, Some(returned), seen.state, true),
                Err("socket_closed") => return report("socket_closed", true, Some(returned), seen.state, already_missed),
                Err(_) => return report("backend_unknown", true, Some(returned), seen.state, already_missed),
            },
            Err("socket_closed") => return report("socket_closed", true, Some(returned), seen.state, already_missed),
            Err(_) => return report("backend_unknown", true, Some(returned), seen.state, already_missed),
        }
        if Instant::now() > deadline {
            return report("still_active", true, Some(returned), seen.state, true);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn report(
    basis: &'static str,
    cancel_invoked: bool,
    cancel_returned: Option<bool>,
    observed_state: String,
    slo_miss: bool,
) -> BackendReport {
    BackendReport {
        basis,
        cancel_invoked,
        cancel_returned,
        observed_state,
        slo_miss,
    }
}

fn owned_row(row: &Activity, application_name: &str, query_role: &str, database: &str) -> bool {
    row.usename == query_role && row.application_name == application_name && row.datname == database
}

struct Activity {
    state: String,
    usename: String,
    application_name: String,
    datname: String,
}

async fn connect(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8]) -> Result<Client, &'static str> {
    let tls = rustls_connector(ca_pem).map_err(|_| "tls")?;
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.dbname(&endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("oo-session-cancel");
    let pending = config.connect(tls);
    let (client, connection) = tokio::time::timeout(Duration::from_secs(3), pending)
        .await
        .map_err(|_| "timeout")?
        .map_err(|_| "connect")?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .batch_execute("SET search_path = pg_catalog; SET statement_timeout = '2s'")
        .await
        .map_err(classify)?;
    Ok(client)
}

async fn privileged(client: &Client, role: &str) -> Result<bool, &'static str> {
    let row = client
        .query_opt("SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = $1", &[&role])
        .await
        .map_err(classify)?;
    let Some(row) = row else {
        return Ok(true);
    };
    row.try_get(0).map_err(|_| "observe")
}

async fn activity(client: &Client, pid: i32) -> Result<Option<Activity>, &'static str> {
    let row = client
        .query_opt(
            "SELECT COALESCE(state, ''), COALESCE(usename, ''), COALESCE(application_name, ''), COALESCE(datname, '') \
             FROM pg_catalog.pg_stat_activity WHERE pid = $1",
            &[&pid],
        )
        .await
        .map_err(classify)?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(Activity {
        state: row.try_get(0).map_err(|_| "observe")?,
        usename: row.try_get(1).map_err(|_| "observe")?,
        application_name: row.try_get(2).map_err(|_| "observe")?,
        datname: row.try_get(3).map_err(|_| "observe")?,
    }))
}

async fn cancel_backend(client: &Client, pid: i32) -> Result<bool, &'static str> {
    let row = client
        .query_one("SELECT pg_catalog.pg_cancel_backend($1)", &[&pid])
        .await
        .map_err(classify)?;
    row.try_get(0).map_err(|_| "observe")
}

async fn still_active_name(
    client: &Client,
    application_name: &str,
    query_role: &str,
    database: &str,
) -> Result<bool, &'static str> {
    let row = client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity \
             WHERE application_name = $1 AND usename = $2 AND datname = $3 AND state = 'active')",
            &[&application_name, &query_role, &database],
        )
        .await
        .map_err(classify)?;
    row.try_get(0).map_err(|_| "observe")
}

fn classify(err: tokio_postgres::Error) -> &'static str {
    if err.is_closed() { "socket_closed" } else { "observe" }
}

#[cfg(test)]
mod tests {
    use super::{confirmable, BackendReport};

    fn sample(basis: &'static str, slo_miss: bool) -> BackendReport {
        BackendReport {
            basis,
            cancel_invoked: true,
            cancel_returned: Some(true),
            observed_state: "active".to_owned(),
            slo_miss,
        }
    }

    #[test]
    fn cancel_return_and_socket_drop_are_not_success() {
        assert!(!confirmable(&sample("socket_closed", false)));
        assert!(!confirmable(&sample("still_active", true)));
        assert!(!confirmable(&sample("backend_unknown", false)));
        assert!(!confirmable(&sample("stopped_after_deadline", true)));
        let mut idle = sample("observed_idle", false);
        idle.cancel_returned = Some(true);
        assert!(confirmable(&idle));
        assert!(!confirmable(&sample("observed_idle", true)));
        assert!(confirmable(&sample("observed_absent", false)));
    }
}
