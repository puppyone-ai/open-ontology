const LIMIT: usize = 4096;

const FORBIDDEN: &[&str] = &[
    "insert", "update", "delete", "drop", "alter", "grant", "revoke", "copy", "set", "reset",
    "truncate", "create", "comment", "vacuum", "analyze", "do", "call", "execute", "prepare",
    "listen", "notify", "lock", "begin", "commit", "rollback", "savepoint",
];

pub fn sql_allowed(sql: &str) -> Result<(), &'static str> {
    let trimmed = sql.trim();
    if trimmed.is_empty() || trimmed.len() > LIMIT {
        return Err("sql_bounds");
    }
    if trimmed.contains(';') || trimmed.contains("--") || trimmed.contains("/*") || trimmed.contains('\0') {
        return Err("sql_shape");
    }
    let lower = trimmed.to_ascii_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    if first != "select" && first != "with" {
        return Err("statement_not_allowed");
    }
    for word in words(&lower) {
        if FORBIDDEN.contains(&word) {
            return Err("statement_not_allowed");
        }
    }
    Ok(())
}

fn words(sql: &str) -> Vec<&str> {
    sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|word| !word.is_empty())
        .collect()
}
