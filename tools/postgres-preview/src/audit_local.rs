//! CSV log classification matching the frozen source-admission verb lists.
//!
//! The live proof still runs `oo-source-admission audit-log` when that binary
//! is present. This copy lets a unit test reject platform COPY and DML before
//! a statement is sent.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditReport {
    pub reader_statements: usize,
    pub forbidden: usize,
    pub other_users_with_forbidden: usize,
    pub proven: bool,
    pub basis: &'static str,
}

const ALLOWED: &[&str] = &[
    "SELECT", "SHOW", "SET", "BEGIN", "COMMIT", "ROLLBACK", "START", "EXPLAIN", "FETCH", "CLOSE",
    "DECLARE", "WITH",
];
const FORBIDDEN: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "MERGE", "TRUNCATE", "CREATE", "ALTER", "DROP", "GRANT",
    "REVOKE", "COMMENT", "COPY", "VACUUM", "ANALYZE", "REINDEX", "CLUSTER", "LOCK", "CALL", "DO",
    "REFRESH", "SECURITY", "IMPORT",
];

pub fn classify_csv(log: &str, reader: &str) -> AuditReport {
    let mut forbidden = 0usize;
    let mut other_forbidden = 0usize;
    let mut statements = 0usize;
    let mut saw_select = false;
    for record in csv_records(log) {
        if record.len() < 14 {
            continue;
        }
        let user = record[1].as_str();
        let Some(sql) = statement_sql(&record[13]) else {
            continue;
        };
        let Some(found) = first_verb(&sql) else {
            if user == reader {
                statements += 1;
                forbidden += 1;
            }
            continue;
        };
        let verb = if found == "WITH" && contains_forbidden_word(&sql) {
            "UNCLASSIFIED"
        } else {
            found
        };
        let bad = FORBIDDEN.contains(&verb) || !ALLOWED.contains(&verb);
        if user == reader {
            statements += 1;
            if verb == "SELECT" {
                saw_select = true;
            }
            if bad {
                forbidden += 1;
            }
        } else if bad {
            other_forbidden += 1;
        }
    }
    let proven = statements > 0 && forbidden == 0 && saw_select;
    AuditReport {
        reader_statements: statements,
        forbidden,
        other_users_with_forbidden: other_forbidden,
        proven,
        basis: if proven {
            "pg_csvlog"
        } else if statements == 0 {
            "audit_empty"
        } else {
            "audit_write"
        },
    }
}

pub fn platform_sql_allowed(sql: &str) -> bool {
    match first_verb(sql) {
        Some(verb) if ALLOWED.contains(&verb) => !(verb == "WITH" && contains_forbidden_word(sql)),
        _ => false,
    }
}

fn contains_forbidden_word(sql: &str) -> bool {
    let mut word = String::new();
    for c in sql.chars() {
        if c.is_ascii_alphabetic() {
            word.push(c.to_ascii_uppercase());
            continue;
        }
        if !word.is_empty() && FORBIDDEN.contains(&word.as_str()) {
            return true;
        }
        word.clear();
    }
    !word.is_empty() && FORBIDDEN.contains(&word.as_str())
}

fn statement_sql(message: &str) -> Option<String> {
    let trimmed = message.trim();
    if let Some(rest) = trimmed.strip_prefix("statement: ") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("execute ")
        && let Some((_, sql)) = rest.split_once(": ")
    {
        return Some(sql.to_string());
    }
    None
}

fn first_verb(sql: &str) -> Option<&'static str> {
    let mut chars = sql.chars().peekable();
    loop {
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
        if chars.peek() == Some(&'(') {
            chars.next();
            continue;
        }
        if chars.peek() == Some(&'-') {
            chars.next();
            if chars.peek() == Some(&'-') {
                while matches!(chars.peek(), Some(c) if *c != '\n') {
                    chars.next();
                }
                continue;
            }
            return None;
        }
        break;
    }
    let mut word = String::new();
    for c in chars {
        if c.is_ascii_alphabetic() {
            word.push(c.to_ascii_uppercase());
        } else {
            break;
        }
    }
    if word.is_empty() {
        return None;
    }
    ALLOWED.iter().chain(FORBIDDEN.iter()).find(|item| **item == word).copied().or(Some("UNCLASSIFIED"))
}

fn csv_records(log: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = log.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' => in_quotes = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\n' => {
                record.push(std::mem::take(&mut field));
                if record.iter().any(|item| !item.is_empty()) {
                    records.push(std::mem::take(&mut record));
                } else {
                    record.clear();
                }
            }
            '\r' => {}
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        if record.iter().any(|item| !item.is_empty()) {
            records.push(record);
        }
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(user: &str, sql: &str) -> String {
        format!("2026-10-10 00:00:00.000 UTC,{user},src,1,[local],s,1,,,0/1,0,LOG,00000,\"statement: {sql}\"\n")
    }

    #[test]
    fn reader_select_is_clean_and_copy_is_not() {
        let log = format!("{}{}", line("reader", "SELECT 1"), line("dba", "COPY public.hubs FROM STDIN"));
        let report = classify_csv(&log, "reader");
        assert!(report.proven);
        assert_eq!(report.forbidden, 0);
        assert_eq!(report.other_users_with_forbidden, 1);
        let dirty = line("reader", "INSERT INTO public.hubs VALUES (1)");
        assert!(!classify_csv(&dirty, "reader").proven);
        assert!(platform_sql_allowed("SELECT count(*) FROM public.hubs"));
        assert!(!platform_sql_allowed("COPY public.hubs TO STDOUT"));
    }

    #[test]
    fn csvlog_without_app_parses_and_savepoint_is_unproven() {
        let select = "2026-10-10 00:00:00.000 UTC,\"oo_dl01w1_46_reader\",src,10,[local],s,1,,,0/1,0,LOG,00000,\"statement: SELECT 1, 2\",,,\n";
        assert!(!select.contains("app="));
        let clean = classify_csv(select, "oo_dl01w1_46_reader");
        assert!(clean.proven);
        assert_eq!(clean.reader_statements, 1);
        assert_eq!(clean.forbidden, 0);
        let savepoint = "2026-10-10 00:00:00.100 UTC,oo_dl01w1_46_reader,src,10,[local],s,2,,,0/1,0,LOG,00000,\"statement: SAVEPOINT _sqlx_savepoint\",,,\n";
        let dirty = classify_csv(savepoint, "oo_dl01w1_46_reader");
        assert!(!dirty.proven);
        assert_eq!(dirty.forbidden, 1);
        assert_eq!(dirty.basis, "audit_write");
        assert!(!platform_sql_allowed("SAVEPOINT _sqlx_savepoint"));
        assert!(!platform_sql_allowed("RELEASE SAVEPOINT _sqlx_savepoint"));
    }
}
