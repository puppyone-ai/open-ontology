//! Sealed probe commands. The probe crate is not edited. A refusal stays a refusal.

use std::path::Path;
use std::time::Duration;

use mariadb_probe::session::{self, Command, Request};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Value as MysqlValue};
use serde_json::{Value, json};

use crate::config::{self, Config, Phase, Secret};

pub async fn run(config: &Config) -> Phase {
    let mut notes = Vec::new();
    let wide = (1000, 8 * 1024 * 1024, 60);
    notes.push(one(config, "read", Command::Read, None, wide, false).await);
    notes.push(
        one(
            config,
            "reject",
            Command::RejectTable,
            Some("k_float_refused"),
            wide,
            false,
        )
        .await,
    );
    notes.push(one(config, "zero-date", Command::ZeroDate, None, wide, false).await);
    notes.push(
        one(
            config,
            "deny-select",
            Command::DenySelect,
            None,
            wide,
            false,
        )
        .await,
    );
    notes.push(
        one(
            config,
            "caps-row",
            Command::CapsRow,
            None,
            (1, 8 * 1024 * 1024, 60),
            false,
        )
        .await,
    );
    notes.push(
        one(
            config,
            "caps-byte",
            Command::CapsByte,
            None,
            (1000, 1, 60),
            false,
        )
        .await,
    );
    notes.push(
        one(
            config,
            "deadline",
            Command::CapsDeadline,
            None,
            (1000, 8 * 1024 * 1024, 1),
            false,
        )
        .await,
    );
    // The deadline probe times out SLEEP(5). That server session can still be in PROCESSLIST.
    // Cancel requires exactly two reader sessions, so reap leftovers before it starts.
    notes.push(match quiet_reader(config).await {
        Ok(()) => one(config, "cancel", Command::Cancel, None, wide, true).await,
        Err(fact) => json!({
            "command": "cancel",
            "ok": false,
            "probe_ok": false,
            "code": "connection_count",
            "fact": fact,
        }),
    });
    let ok = notes
        .iter()
        .all(|note| note.get("ok").and_then(Value::as_bool) == Some(true));
    let phase = Phase::new("preflight", ok, json!({ "commands": notes }));
    let _ = phase.save(&config.evidence);
    phase
}

pub async fn broken(config: &Config) -> Phase {
    let note = one(
        config,
        "broken",
        Command::Broken,
        None,
        (1000, 8 * 1024 * 1024, 60),
        false,
    )
    .await;
    let ok = note.get("ok").and_then(Value::as_bool) == Some(true);
    let phase = Phase::new("broken", ok, note);
    let _ = phase.save(&config.evidence);
    phase
}

async fn one(
    config: &Config,
    name: &str,
    command: Command,
    table: Option<&str>,
    limits: (u64, u64, u64),
    go_file: bool,
) -> Value {
    let (max_rows, max_bytes, deadline_secs) = limits;
    let out = config.evidence.join("probe").join(name);
    let ready = if matches!(command, Command::Broken) {
        config.broken_ready.clone()
    } else {
        out.join("ready.signal")
    };
    let go = out.join("go.signal");
    if go_file {
        let ready_watch = ready.clone();
        let go_write = go.clone();
        tokio::spawn(async move {
            for _ in 0..300 {
                if ready_watch.is_file() {
                    let _ = std::fs::write(&go_write, b"go\n");
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
    }
    let user = match Secret::load(&config.maria_user_file) {
        Ok(user) => user.expose().to_string(),
        Err(err) => return json!({ "command": name, "ok": false, "code": "secret", "fact": err }),
    };
    let request = Request {
        host: config.maria_host.clone(),
        port: config.maria_port,
        user,
        password_file: config.maria_password_file.clone(),
        out: out.clone(),
        command,
        table: table.map(str::to_string),
        ready_file: (go_file || matches!(command, Command::Broken)).then_some(ready),
        go_file: go_file.then_some(go),
        max_rows,
        max_bytes,
        deadline: Duration::from_secs(deadline_secs),
        batch_rows: 4,
        image_digest: session::IMAGE_DIGEST.to_string(),
    };
    if config.image_digest != session::IMAGE_DIGEST {
        return json!({
            "command": name,
            "ok": false,
            "code": "image_digest",
            "fact": "config digest does not match the sealed probe image"
        });
    }
    let result = session::execute(request).await;
    judge(name, &out, result)
}

fn judge(name: &str, out: &Path, result: Result<(), mariadb_probe::fail::Fail>) -> Value {
    let (code, fact, probe_ok) = match &result {
        Ok(()) => (None, None, true),
        Err(err) => (Some(err.code), Some(err.fact.as_str()), false),
    };
    let ok = match name {
        "read" => probe_ok,
        "reject" => matches!(code, Some(code) if code != "expected_refusal" && code != "table"),
        "zero-date" => probe_ok && file_state(out, "zero-date.json", "negative_proven"),
        "deny-select" => probe_ok && file_state(out, "deny-select.json", "negative_proven"),
        "caps-row" => code == Some("row_cap"),
        "caps-byte" => code == Some("byte_cap"),
        "deadline" => {
            code == Some("deadline") && fact != Some("sleep finished inside the deadline")
        }
        "cancel" => probe_ok && file_state(out, "cancel.json", "cancel_proven"),
        "broken" => {
            code == Some("broken_connection")
                && fact.is_some_and(|fact| !fact.contains("sleep completed"))
        }
        _ => false,
    };
    json!({
        "command": name,
        "ok": ok,
        "probe_ok": probe_ok,
        "code": code,
        "fact": fact.map(|text| config::redact(text, &[])),
    })
}

async fn quiet_reader(config: &Config) -> Result<(), String> {
    let user = Secret::load(&config.maria_user_file)?;
    let password = Secret::load(&config.maria_password_file)?;
    let secrets = [password.expose()];
    let name = user.expose();
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("reader name".to_string());
    }
    let opts = OptsBuilder::default()
        .ip_or_hostname(config.maria_host.clone())
        .tcp_port(config.maria_port)
        .user(Some(name.to_string()))
        .pass(Some(password.expose().to_string()))
        .db_name(Some(crate::cell::SCHEMA.to_string()))
        .prefer_socket(false)
        .stmt_cache_size(0);
    let connected = tokio::time::timeout(Duration::from_secs(15), Conn::new(opts)).await;
    let mut conn = match connected {
        Ok(Ok(conn)) => conn,
        Ok(Err(err)) => return Err(config::redact(&err.to_string(), &secrets)),
        Err(_) => return Err("reader quiet timeout".to_string()),
    };
    let outcome = reap_other_sessions(&mut conn, name, &secrets).await;
    let _ = conn.disconnect().await;
    outcome
}

async fn reap_other_sessions(conn: &mut Conn, name: &str, secrets: &[&str]) -> Result<(), String> {
    let count_sql =
        format!("SELECT COUNT(*) FROM information_schema.PROCESSLIST WHERE USER = '{name}'");
    let id_sql = format!(
        "SELECT ID FROM information_schema.PROCESSLIST WHERE USER = '{name}' AND ID <> CONNECTION_ID()"
    );
    let mut killed = false;
    for step in 0..80 {
        let count = match one_u64(conn, &count_sql).await {
            Ok(count) => count,
            Err(err) => return Err(config::redact(&err, secrets)),
        };
        if count == 1 {
            return Ok(());
        }
        if step == 20 && !killed {
            let ids = match other_ids(conn, &id_sql).await {
                Ok(ids) => ids,
                Err(err) => return Err(config::redact(&err, secrets)),
            };
            for id in ids {
                if let Err(err) = conn.query_drop(format!("KILL QUERY {id}")).await {
                    let fact = err.to_string();
                    if !fact.contains("1094") {
                        return Err(config::redact(&fact, secrets));
                    }
                }
            }
            killed = true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("reader sessions remained".to_string())
}

async fn one_u64(conn: &mut Conn, sql: &str) -> Result<u64, String> {
    let mut rows: Vec<mysql_async::Row> = conn.query(sql).await.map_err(|err| err.to_string())?;
    let mut row = rows.pop().ok_or_else(|| "count missing".to_string())?;
    let value = row.take(0).ok_or_else(|| "count column".to_string())?;
    u64_value(value)
}

async fn other_ids(conn: &mut Conn, sql: &str) -> Result<Vec<u64>, String> {
    let rows: Vec<mysql_async::Row> = conn.query(sql).await.map_err(|err| err.to_string())?;
    let mut ids = Vec::with_capacity(rows.len());
    for mut row in rows {
        let value = row.take(0).ok_or_else(|| "id column".to_string())?;
        ids.push(u64_value(value)?);
    }
    Ok(ids)
}

fn u64_value(value: MysqlValue) -> Result<u64, String> {
    match value {
        MysqlValue::UInt(number) => Ok(number),
        MysqlValue::Int(number) if number >= 0 => {
            u64::try_from(number).map_err(|_| "count range".to_string())
        }
        MysqlValue::Bytes(bytes) => {
            let text = String::from_utf8(bytes).map_err(|_| "count utf8".to_string())?;
            text.parse().map_err(|_| "count parse".to_string())
        }
        _ => Err("count shape".to_string()),
    }
}

fn file_state(out: &Path, name: &str, state: &str) -> bool {
    let Ok(bytes) = std::fs::read(out.join(name)) else {
        return false;
    };
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| {
            value
                .get("state")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .as_deref()
        == Some(state)
}
