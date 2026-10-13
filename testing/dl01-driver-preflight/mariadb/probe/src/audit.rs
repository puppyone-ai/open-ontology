use crate::fail::Fail;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditCounts {
    pub rows: u32,
    pub connect: u32,
    pub read: u32,
    pub session: u32,
    pub cancel: u32,
    pub execute: u32,
    pub write: u32,
    pub empty: u32,
    pub unexpected: u32,
}

pub fn audit_log(text: &str) -> Result<AuditCounts, Fail> {
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let (command, argument) = match line.split_once('\t') {
            Some((command, argument)) => (command.to_string(), argument.to_string()),
            None => (line.to_string(), String::new()),
        };
        rows.push((command, argument));
    }
    let prepares_are_reads = rows.iter().filter(|(command, _)| command == "Prepare").all(|(_, argument)| {
        matches!(classify_sql(argument), SqlClass::Read)
    });
    let mut counts = AuditCounts::default();
    for (command, argument) in &rows {
        counts.rows += 1;
        let class = classify_command(command, argument, prepares_are_reads);
        match class {
            "connect" => counts.connect += 1,
            "read" => counts.read += 1,
            "session" => counts.session += 1,
            "cancel" => counts.cancel += 1,
            "execute" => counts.execute += 1,
            "write" => counts.write += 1,
            "empty" => counts.empty += 1,
            _ => counts.unexpected += 1,
        }
    }
    if counts.rows == 0 || counts.read == 0 || counts.connect == 0 || counts.write != 0 || counts.empty != 0 || counts.unexpected != 0 {
        return Err(Fail::new(
            "write_audit",
            format!(
                "rows={} connect={} read={} session={} cancel={} execute={} write={} empty={} unexpected={}",
                counts.rows, counts.connect, counts.read, counts.session, counts.cancel, counts.execute, counts.write, counts.empty, counts.unexpected
            ),
        ));
    }
    Ok(counts)
}

fn classify_command(command: &str, argument: &str, empty_execute_ok: bool) -> &'static str {
    match command {
        "Connect" | "Quit" | "Init DB" | "Close stmt" | "Reset stmt" => "connect",
        "Prepare" => match classify_sql(argument) {
            SqlClass::Read => "read",
            SqlClass::Session => "session",
            SqlClass::Cancel => "cancel",
            SqlClass::Write => "write",
            SqlClass::Empty => "empty",
            SqlClass::Unexpected => "unexpected",
        },
        "Execute" => {
            if argument.trim().is_empty() {
                if empty_execute_ok { "execute" } else { "unexpected" }
            } else {
                match classify_sql(argument) {
                    SqlClass::Read => "read",
                    SqlClass::Session => "session",
                    SqlClass::Cancel => "cancel",
                    SqlClass::Write => "write",
                    SqlClass::Empty => "empty",
                    SqlClass::Unexpected => "unexpected",
                }
            }
        }
        "Query" => match classify_sql(argument) {
            SqlClass::Read => "read",
            SqlClass::Session => "session",
            SqlClass::Cancel => "cancel",
            SqlClass::Write => "write",
            SqlClass::Empty => "empty",
            SqlClass::Unexpected => "unexpected",
        },
        _ => "unexpected",
    }
}

enum SqlClass {
    Read,
    Session,
    Cancel,
    Write,
    Empty,
    Unexpected,
}

fn classify_sql(argument: &str) -> SqlClass {
    let sql = strip_comments(argument);
    if sql.is_empty() {
        return SqlClass::Empty;
    }
    let upper = sql.to_ascii_uppercase();
    if upper.starts_with("KILL") {
        return if is_query_cancel(&upper) { SqlClass::Cancel } else { SqlClass::Write };
    }
    if write_verb(&upper) {
        return SqlClass::Write;
    }
    let verb = upper.split_whitespace().next().unwrap_or("");
    match verb {
        "SELECT" | "SHOW" | "WITH" | "EXPLAIN" | "DESCRIBE" | "DESC" | "USE" => SqlClass::Read,
        "SET" => {
            if is_session_set(&upper) {
                SqlClass::Session
            } else {
                SqlClass::Write
            }
        }
        _ => SqlClass::Unexpected,
    }
}

fn strip_comments(sql: &str) -> String {
    let mut out = String::new();
    let bytes = sql.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'/' && index + 1 < bytes.len() && bytes[index + 1] == b'*' {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            out.push(' ');
            continue;
        }
        if bytes[index] == b'-' && index + 1 < bytes.len() && bytes[index + 1] == b'-' {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            out.push(' ');
            continue;
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_query_cancel(upper: &str) -> bool {
    let mut parts = upper.split_whitespace();
    parts.next() == Some("KILL") && parts.next() == Some("QUERY") && parts.next().is_some_and(|token| token.bytes().all(|byte| byte.is_ascii_digit())) && parts.next().is_none()
}

fn write_verb(upper: &str) -> bool {
    const WORDS: &[&str] = &[
        "INSERT", "UPDATE", "DELETE", "REPLACE", "ALTER", "CREATE", "DROP", "TRUNCATE", "GRANT", "REVOKE", "RENAME",
        "LOAD", "CALL", "HANDLER", "OPTIMIZE", "REPAIR", "INSTALL", "UNINSTALL", "FLUSH", "RESET", "PURGE", "BINLOG",
    ];
    upper.split(|ch: char| !ch.is_ascii_alphanumeric()).any(|word| WORDS.contains(&word))
        || upper.contains("INTO OUTFILE")
        || upper.contains("INTO DUMPFILE")
}

fn is_session_set(upper: &str) -> bool {
    if upper.contains("GLOBAL") {
        return false;
    }
    let rest = upper.trim_start_matches("SET").trim();
    let rest = rest.strip_prefix("SESSION").unwrap_or(rest).trim();
    rest.starts_with("NAMES") || rest.starts_with("TIME_ZONE")
}
