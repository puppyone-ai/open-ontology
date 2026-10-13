use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;

use crate::audit::Audit;
use crate::fail::Fail;
use crate::session::{self, Endpoint, Link};
use crate::sqltext;

const OWNED_DB: &str = "oo_dl01w1_43";

struct Phase {
    name: &'static str,
    file: &'static str,
    expect_error: bool,
    observational: bool,
}

const PHASES: &[Phase] = &[
    Phase { name: "schema", file: "schema.sql", expect_error: false, observational: false },
    Phase { name: "load", file: "load.sql", expect_error: false, observational: false },
    Phase { name: "mutate", file: "mutate.sql", expect_error: false, observational: false },
    Phase { name: "negative-varbinary", file: "negative-varbinary.sql", expect_error: true, observational: false },
    Phase { name: "negative-primary", file: "negative-primary.sql", expect_error: true, observational: false },
    Phase { name: "negative-streams", file: "negative-streams.sql", expect_error: true, observational: false },
    Phase { name: "negative-range", file: "negative-range.sql", expect_error: true, observational: false },
    Phase { name: "negative-partition", file: "negative-partition.sql", expect_error: true, observational: false },
    Phase { name: "saturation", file: "saturation.sql", expect_error: false, observational: true },
    Phase { name: "nul", file: "nul.sql", expect_error: false, observational: true },
];

pub async fn setup(
    evidence: PathBuf,
    sql_dir: &str,
    secrets: &str,
    endpoint: Endpoint,
    be_host: &str,
    be_port: u16,
) -> Result<(), Fail> {
    if endpoint.db != OWNED_DB {
        return Err(Fail::new("db", OWNED_DB));
    }
    let secrets_dir = Path::new(secrets);
    let bootstrap = session::load_secret(&secrets_dir.join("root-bootstrap.password"), true)?;
    let root = session::load_secret(&secrets_dir.join("root.password"), false)?;
    let dba_password = session::load_secret(&secrets_dir.join("dba.password"), false)?;
    let reader = session::load_secret(&secrets_dir.join("reader.password"), false)?;
    let audit = Arc::new(Audit::create(
        &evidence.join("dba-audit.jsonl"),
        vec![root.to_string(), dba_password.to_string(), reader.to_string()],
    )?);
    let mut bootstrap_endpoint = endpoint.clone();
    bootstrap_endpoint.db.clear();
    let mut link = session::connect_retry(
        &bootstrap_endpoint,
        "root",
        bootstrap.as_str(),
        &audit,
        48,
        &evidence.join("connect-retry.log"),
    )
    .await?;
    let add = format!("ALTER SYSTEM ADD BACKEND \"{be_host}:{be_port}\"");
    match link.exec(&add, &audit).await {
        Ok(()) => {}
        Err(err) if err.fact.to_ascii_lowercase().contains("already exists") => {}
        Err(err) => return fail_close(link, &audit, err).await,
    }
    if let Err(err) = wait_backend(&mut link, &audit).await {
        return fail_close(link, &audit, err).await;
    }
    let sql_dir = PathBuf::from(sql_dir);
    let mut phase_notes = Vec::new();
    for phase in PHASES {
        let sql = std::fs::read_to_string(sql_dir.join(phase.file))
            .map_err(|err| Fail::new("sql", format!("{} {err}", phase.file)))?;
        let result = run_script(&mut link, &audit, &sql).await;
        let note = match (result, phase.expect_error, phase.observational) {
            (Ok(()), false, _) => json!({"phase": phase.name, "ok": true}),
            (Err(err), true, _) => json!({"phase": phase.name, "ok": false, "expected_error": true, "error": err.fact}),
            (Err(err), false, true) => json!({"phase": phase.name, "ok": false, "observational": true, "error": err.fact}),
            (Ok(()), true, _) => {
                return fail_close(link, &audit, Fail::new("expected_error", phase.name)).await;
            }
            (Err(err), false, false) => return fail_close(link, &audit, err).await,
        };
        phase_notes.push(note);
    }
    if let Err(err) = expect_count(&mut link, &audit, "part_day", 3).await {
        return fail_close(link, &audit, err).await;
    }
    if let Err(err) = expect_count(&mut link, &audit, "dup_events", 5).await {
        return fail_close(link, &audit, err).await;
    }
    let grants = user_sql(reader.as_str(), dba_password.as_str(), root.as_str());
    if let Err(err) = run_script(&mut link, &audit, &grants).await {
        return fail_close(link, &audit, err).await;
    }
    link.close(&audit).await;
    let reader_link = session::connect_retry(
        &endpoint,
        "oo_reader",
        reader.as_str(),
        &audit,
        6,
        &evidence.join("reader-ping-retry.log"),
    )
    .await?;
    let mut reader_link = reader_link;
    if let Err(err) = reader_link.exec("SELECT 1", &audit).await {
        return fail_close(reader_link, &audit, err).await;
    }
    reader_link.close(&audit).await;
    let empty = session::connect_once(&endpoint, "root", "", &audit).await;
    let empty_rejected = empty.is_err();
    if let Ok(link) = empty {
        link.close(&audit).await;
    }
    if !empty_rejected {
        return Err(Fail::new("root_password", "empty password still accepted"));
    }
    session::write_json(
        &evidence.join("dba-ready.json"),
        &json!({
            "protocol": "oo.doris.dba-setup/v0",
            "database": OWNED_DB,
            "phases": phase_notes,
            "empty_root_rejected": true,
            "source_acceptance": false
        }),
    )?;
    Ok(())
}

async fn wait_backend(link: &mut Link, audit: &Audit) -> Result<(), Fail> {
    for _ in 0..80 {
        let rows = link.query("SHOW BACKENDS", audit).await?;
        if backends_alive(&rows)? {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    Err(Fail::new("backend", "not alive"))
}

fn backends_alive(rows: &[mysql_async::Row]) -> Result<bool, Fail> {
    if rows.is_empty() {
        return Ok(false);
    }
    let alive_at = session::column_index(&rows[0], &["alive"]);
    for row in rows {
        let text = if let Some(index) = alive_at {
            session::text_at(row, index)?
        } else {
            session::row_texts(row)?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ")
        };
        if text.eq_ignore_ascii_case("true") || text.to_ascii_lowercase().contains("true") {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn run_script(link: &mut Link, audit: &Audit, sql: &str) -> Result<(), Fail> {
    for statement in sqltext::split(sql) {
        link.exec(&statement, audit).await?;
    }
    Ok(())
}

async fn expect_count(link: &mut Link, audit: &Audit, table: &str, expected: i64) -> Result<(), Fail> {
    session::ident(table)?;
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let rows = link.query(&sql, audit).await?;
    let got = rows
        .first()
        .ok_or_else(|| Fail::new("count", table))
        .and_then(|row| session::text_at(row, 0))?
        .parse::<i64>()
        .map_err(|_| Fail::new("count", table))?;
    if got == expected {
        Ok(())
    } else {
        Err(Fail::new("count", format!("{table} {got} != {expected}")))
    }
}

fn user_sql(reader: &str, dba_password: &str, root: &str) -> String {
    format!(
        "CREATE USER 'oo_reader' IDENTIFIED BY '{reader}';\n\
         GRANT SELECT_PRIV ON internal.{OWNED_DB}.* TO 'oo_reader';\n\
         CREATE USER 'oo_dba' IDENTIFIED BY '{dba_password}';\n\
         GRANT ADMIN_PRIV ON *.*.* TO 'oo_dba';\n\
         ALTER USER 'root' IDENTIFIED BY '{root}';\n"
    )
}

async fn fail_close(link: Link, audit: &Audit, err: Fail) -> Result<(), Fail> {
    link.close(audit).await;
    Err(err)
}
