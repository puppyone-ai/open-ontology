use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row};
use tokio::time::sleep;
use zeroize::Zeroizing;

use crate::audit::Audit;
use crate::cell;
use crate::fail::Fail;

#[derive(Clone)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub db: String,
    pub reader_user: String,
}

pub struct Link {
    pub conn: Conn,
    pub user: String,
    pub database: String,
    pub id: u32,
}

pub fn ident(value: &str) -> Result<(), Fail> {
    if (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        Ok(())
    } else {
        Err(Fail::new("ident", "rejected"))
    }
}

pub fn ipv4(value: &str) -> Result<(), Fail> {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|part| part.parse::<u8>().is_ok()) {
        Ok(())
    } else {
        Err(Fail::new("host", "ipv4"))
    }
}

pub fn load_secret(path: &Path, allow_empty: bool) -> Result<Zeroizing<String>, Fail> {
    let meta = fs::metadata(path).map_err(|err| Fail::new("secret", err.to_string()))?;
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(Fail::new("secret_mode", format!("{mode:o}")));
    }
    let text = fs::read_to_string(path).map_err(|err| Fail::new("secret", err.to_string()))?;
    let trimmed = text.trim_end_matches(['\n', '\r']).to_string();
    if !allow_empty && trimmed.is_empty() {
        return Err(Fail::new("secret", "empty"));
    }
    if !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(Fail::new("secret", "charset"));
    }
    if !allow_empty && trimmed.len() < 8 {
        return Err(Fail::new("secret", "short"));
    }
    Ok(Zeroizing::new(trimmed))
}

pub async fn connect_retry(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    audit: &Audit,
    attempts: u32,
    log: &Path,
) -> Result<Link, Fail> {
    let mut last = Fail::new("connect", "no attempt");
    for attempt in 1..=attempts {
        match connect_once(endpoint, user, password, audit).await {
            Ok(link) => return Ok(link),
            Err(err) => {
                last = err;
                let _ = append_log(log, &format!("attempt {attempt} {last}\n"));
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
    Err(last)
}

pub async fn connect_once(endpoint: &Endpoint, user: &str, password: &str, audit: &Audit) -> Result<Link, Fail> {
    let mut opts = OptsBuilder::default()
        .ip_or_hostname(endpoint.host.clone())
        .tcp_port(endpoint.port)
        .user(Some(user))
        .pass(Some(password))
        .prefer_socket(Some(false))
        .stmt_cache_size(Some(0usize));
    if !endpoint.db.is_empty() {
        opts = opts.db_name(Some(endpoint.db.clone()));
    }
    let conn = Conn::new(opts)
        .await
        .map_err(|err| Fail::new("connect", err.to_string()))?;
    let id = conn.id();
    audit.handshake(user, id);
    Ok(Link {
        conn,
        user: user.to_string(),
        database: endpoint.db.clone(),
        id,
    })
}

impl Link {
    pub async fn query(&mut self, sql: &str, audit: &Audit) -> Result<Vec<Row>, Fail> {
        match self.conn.query_iter(sql).await {
            Ok(mut result) => {
                let mut rows = Vec::new();
                loop {
                    match result.next().await {
                        Ok(Some(row)) => rows.push(row),
                        Ok(None) => break,
                        Err(err) => {
                            let message = err.to_string();
                            audit.statement(&self.user, self.id, sql, false, Some(message.clone()));
                            let _ = result.drop_result().await;
                            return Err(Fail::new("query", message));
                        }
                    }
                }
                if let Err(err) = result.drop_result().await {
                    let message = err.to_string();
                    audit.statement(&self.user, self.id, sql, false, Some(message.clone()));
                    return Err(Fail::new("query", message));
                }
                audit.statement(&self.user, self.id, sql, true, None);
                Ok(rows)
            }
            Err(err) => {
                let message = err.to_string();
                audit.statement(&self.user, self.id, sql, false, Some(message.clone()));
                Err(Fail::new("query", message))
            }
        }
    }

    pub async fn query_bound(
        &mut self,
        sql: &str,
        audit: &Audit,
        max_rows: Option<usize>,
        max_bytes: Option<usize>,
    ) -> Result<(Vec<Row>, Option<&'static str>, usize), Fail> {
        let mut result = self
            .conn
            .query_iter(sql)
            .await
            .map_err(|err| {
                let message = err.to_string();
                audit.statement(&self.user, self.id, sql, false, Some(message.clone()));
                Fail::new("query", message)
            })?;
        let mut rows = Vec::new();
        let mut bytes = 0usize;
        let mut reason = None;
        loop {
            match result.next().await {
                Ok(Some(row)) => {
                    let width = row_texts(&row)?.iter().map(|cell| cell.as_ref().map(String::len).unwrap_or(0)).sum::<usize>();
                    if max_rows.is_some_and(|max| rows.len() >= max) {
                        reason = Some("max_rows");
                        break;
                    }
                    if max_bytes.is_some_and(|max| bytes.saturating_add(width) > max) {
                        reason = Some("max_bytes");
                        break;
                    }
                    bytes += width;
                    rows.push(row);
                }
                Ok(None) => break,
                Err(err) => {
                    let message = err.to_string();
                    audit.statement(&self.user, self.id, sql, false, Some(message.clone()));
                    let _ = result.drop_result().await;
                    return Err(Fail::new("query", message));
                }
            }
        }
        if let Err(err) = result.drop_result().await {
            let message = err.to_string();
            audit.statement(&self.user, self.id, sql, false, Some(message.clone()));
            return Err(Fail::new("query", message));
        }
        audit.statement(&self.user, self.id, sql, true, None);
        Ok((rows, reason, bytes))
    }

    pub async fn exec(&mut self, sql: &str, audit: &Audit) -> Result<(), Fail> {
        match self.conn.query_drop(sql).await {
            Ok(()) => {
                audit.statement(&self.user, self.id, sql, true, None);
                Ok(())
            }
            Err(err) => {
                let message = err.to_string();
                audit.statement(&self.user, self.id, sql, false, Some(message.clone()));
                Err(Fail::new("exec", message))
            }
        }
    }

    pub async fn close(self, audit: &Audit) {
        let id = self.id;
        let user = self.user.clone();
        let _ = self.conn.disconnect().await;
        audit.quit(&user, id);
    }
}

pub fn row_texts(row: &Row) -> Result<Vec<Option<String>>, Fail> {
    let mut cells = Vec::with_capacity(row.len());
    for index in 0..row.len() {
        let value = row.as_ref(index).ok_or_else(|| Fail::new("cell", "missing"))?;
        cells.push(cell::as_optional_text(value)?);
    }
    Ok(cells)
}

pub fn rows_texts(rows: &[Row]) -> Result<Vec<Vec<Option<String>>>, Fail> {
    rows.iter().map(row_texts).collect()
}

pub fn text_at(row: &Row, index: usize) -> Result<String, Fail> {
    let value = row.as_ref(index).ok_or_else(|| Fail::new("cell", "missing"))?;
    cell::as_text(value)
}

pub fn column_index(row: &Row, names: &[&str]) -> Option<usize> {
    row.columns_ref().iter().position(|column| {
        let name = column.name_str().to_ascii_lowercase();
        names.iter().any(|want| name == *want)
    })
}

pub fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), Fail> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| Fail::new("write", err.to_string()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|err| Fail::new("json", err.to_string()))?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|err| Fail::new("write", err.to_string()))
}

pub fn append_log(path: &Path, line: &str) -> Result<(), Fail> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| Fail::new("write", err.to_string()))?;
    }
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| Fail::new("write", err.to_string()))?;
    file.write_all(line.as_bytes())
        .map_err(|err| Fail::new("write", err.to_string()))
}

pub fn evidence_dir(path: &str) -> Result<PathBuf, Fail> {
    let path = PathBuf::from(path);
    fs::create_dir_all(&path).map_err(|err| Fail::new("evidence", err.to_string()))?;
    Ok(path)
}
