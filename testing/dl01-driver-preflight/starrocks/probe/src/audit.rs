use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::fail::Fail;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlMode {
    ReadOnly,
    Cancel,
}

#[derive(Debug, Clone)]
struct Event {
    role: String,
    class: String,
    sql: Option<String>,
    connection_id: Option<u32>,
}

#[derive(Clone, Default)]
pub struct Audit {
    inner: Arc<Mutex<Vec<Event>>>,
}

impl Audit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handshake(&self, role: &str, connection_id: u32) {
        self.push(role, "handshake", None, Some(connection_id));
    }

    pub fn cleanup(&self, role: &str, connection_id: u32) {
        self.push(role, "cleanup", None, Some(connection_id));
    }

    pub fn sql(&self, role: &str, sql: &str, mode: SqlMode) -> Result<&'static str, Fail> {
        let class = allow_sql(sql, mode)?;
        self.push(role, class, Some(sql.to_string()), None);
        Ok(class)
    }

    pub fn note_connection(&self, role: &str, class: &str, connection_id: u32) {
        let mut events = self.lock();
        if let Some(event) = events.iter_mut().rev().find(|event| event.role == role && event.class == class && event.connection_id.is_none()) {
            event.connection_id = Some(connection_id);
        }
    }

    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        for (index, event) in self.lock().iter().enumerate() {
            let line = json!({
                "seq": index + 1,
                "role": event.role,
                "class": event.class,
                "sql": event.sql,
                "connection_id": event.connection_id,
            });
            out.push_str(&line.to_string());
            out.push('\n');
        }
        out
    }

    pub fn counts(&self) -> Value {
        let mut handshake = 0u64;
        let mut read = 0u64;
        let mut session = 0u64;
        let mut cancel = 0u64;
        let mut cleanup = 0u64;
        let mut write = 0u64;
        let mut unexpected = 0u64;
        let events = self.lock();
        for event in events.iter() {
            match event.class.as_str() {
                "handshake" => handshake += 1,
                "read" => read += 1,
                "session" => session += 1,
                "cancel" => cancel += 1,
                "cleanup" => cleanup += 1,
                "write" => write += 1,
                _ => unexpected += 1,
            }
        }
        json!({
            "rows": events.len(),
            "handshake": handshake,
            "read": read,
            "session": session,
            "cancel": cancel,
            "cleanup": cleanup,
            "write": write,
            "unexpected": unexpected,
        })
    }

    fn push(&self, role: &str, class: &str, sql: Option<String>, connection_id: Option<u32>) {
        self.lock().push(Event {
            role: role.to_string(),
            class: class.to_string(),
            sql,
            connection_id,
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Event>> {
        self.inner.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}

pub fn allow_sql(sql: &str, mode: SqlMode) -> Result<&'static str, Fail> {
    let trimmed = sql.trim();
    if trimmed.is_empty() || trimmed.contains(';') || trimmed.contains("--") || trimmed.contains("/*") {
        return Err(Fail::new("unexpected_sql", "empty or compound statement"));
    }
    let upper = trimmed.to_ascii_uppercase();
    if write_keyword(&upper) {
        return Err(Fail::new("reader_write_blocked", trimmed.to_string()));
    }
    if upper.starts_with("SELECT ") {
        return Ok("read");
    }
    if upper.starts_with("SHOW ") || upper.starts_with("DESC ") || upper.starts_with("DESCRIBE ") {
        return Ok("read");
    }
    if upper == "SET QUERY_TIMEOUT = 120" || upper == "SET QUERY_TIMEOUT = 180" || upper == "USE OO_W45_SR" {
        return Ok("session");
    }
    if matches!(mode, SqlMode::Cancel) && kill_query(&upper) {
        return Ok("cancel");
    }
    Err(Fail::new("unexpected_sql", trimmed.to_string()))
}

fn write_keyword(upper: &str) -> bool {
    const WORDS: &[&str] = &[
        "INSERT", "UPDATE", "DELETE", "REPLACE", "CREATE", "ALTER", "DROP", "TRUNCATE", "GRANT",
        "REVOKE", "LOAD", "IMPORT", "ADMIN", "RESTORE", "EXPORT", "OPERATE",
    ];
    let first = upper.split_whitespace().next().unwrap_or("");
    WORDS.contains(&first)
}

fn kill_query(upper: &str) -> bool {
    let mut parts = upper.split_whitespace();
    if parts.next() != Some("KILL") || parts.next() != Some("QUERY") {
        return false;
    }
    matches!(
        parts.next(),
        Some(id) if id.chars().all(|item| item.is_ascii_digit()) && parts.next().is_none()
    )
}

#[cfg(test)]
mod tests {
    use super::{allow_sql, SqlMode};

    #[test]
    fn reader_statements_are_classified_and_writes_blocked() {
        assert_eq!(allow_sql("SELECT current_version()", SqlMode::ReadOnly).unwrap(), "read");
        assert_eq!(allow_sql("SHOW GRANTS FOR 'oo_w45_reader'@'%'", SqlMode::ReadOnly).unwrap(), "read");
        assert_eq!(allow_sql("SHOW FULL PROCESSLIST", SqlMode::Cancel).unwrap(), "read");
        assert_eq!(allow_sql("SET query_timeout = 180", SqlMode::ReadOnly).unwrap(), "session");
        assert_eq!(allow_sql("USE oo_w45_sr", SqlMode::ReadOnly).unwrap(), "session");
        assert_eq!(allow_sql("KILL QUERY 15", SqlMode::Cancel).unwrap(), "cancel");
        assert!(allow_sql("KILL QUERY 15", SqlMode::ReadOnly).is_err());
        assert!(allow_sql("INSERT INTO pk_accounts VALUES (1)", SqlMode::ReadOnly).is_err());
        assert!(allow_sql("SELECT 1; SELECT 2", SqlMode::ReadOnly).is_err());
    }
}
