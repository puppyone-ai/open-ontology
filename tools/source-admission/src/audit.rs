//! PostgreSQL csvlog classification. An empty reader log is not zero-write.

use serde::Serialize;

use crate::model::AUDIT_PROTOCOL;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuditReport {
    pub protocol: String,
    pub reader: String,
    pub reader_statements: usize,
    pub reader_verbs: Vec<String>,
    pub forbidden: usize,
    pub other_users_with_forbidden: usize,
    pub zero_write: AuditZeroWrite,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuditZeroWrite {
    pub proven: bool,
    pub basis: String,
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
    let mut verbs = Vec::new();
    let mut forbidden = 0usize;
    let mut other_forbidden = 0usize;
    let mut statements = 0usize;
    for record in csv_records(log) {
        if record.len() < 14 {
            continue;
        }
        let user = record[1].as_str();
        let message = record[13].as_str();
        let Some(sql) = statement_sql(message) else {
            continue;
        };
        let Some(found) = first_verb(&sql) else {
            if user == reader {
                statements += 1;
                forbidden += 1;
                verbs.push("UNCLASSIFIED".to_string());
            }
            continue;
        };
        // A data-modifying CTE starts with WITH and still contains a write verb.
        let verb = if found == "WITH" && contains_forbidden_word(&sql) {
            "UNCLASSIFIED"
        } else {
            found
        };
        let bad = FORBIDDEN.contains(&verb) || !ALLOWED.contains(&verb);
        if user == reader {
            statements += 1;
            verbs.push(verb.to_string());
            if bad {
                forbidden += 1;
            }
        } else if bad {
            other_forbidden += 1;
        }
    }
    let proven = statements > 0 && forbidden == 0 && verbs.iter().any(|verb| verb == "SELECT");
    AuditReport {
        protocol: AUDIT_PROTOCOL.to_string(),
        reader: reader.to_string(),
        reader_statements: statements,
        reader_verbs: verbs,
        forbidden,
        other_users_with_forbidden: other_forbidden,
        zero_write: AuditZeroWrite {
            proven,
            basis: if proven {
                "pg_csvlog".to_string()
            } else if statements == 0 {
                "audit_empty".to_string()
            } else {
                "audit_write".to_string()
            },
        },
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
    ALLOWED
        .iter()
        .chain(FORBIDDEN.iter())
        .find(|item| **item == word)
        .copied()
        .or(Some("UNCLASSIFIED"))
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
            ',' => {
                record.push(std::mem::take(&mut field));
            }
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
    //! mock: classifier fixtures only. No database and no server log file.
    use super::*;

    fn line(user: &str, sql: &str) -> String {
        format!(
            "2026-10-09 00:00:00.000 UTC,{user},src,1,[local],s,1,,,0/1,0,LOG,00000,\"statement: {sql}\"\n"
        )
    }

    #[test]
    fn mock_reader_select_is_proven_and_dba_insert_is_not_attributed() {
        let log =
            line("oo_reader", "SELECT 1") + &line("oo_dba", "INSERT INTO app.item VALUES (1, 'x')");
        let report = classify_csv(&log, "oo_reader");
        assert!(report.zero_write.proven);
        assert_eq!(report.forbidden, 0);
        assert_eq!(report.other_users_with_forbidden, 1);
        assert_eq!(report.zero_write.basis, "pg_csvlog");
    }

    #[test]
    fn mock_empty_log_is_not_proof() {
        let report = classify_csv("", "oo_reader");
        assert!(!report.zero_write.proven);
        assert_eq!(report.zero_write.basis, "audit_empty");
    }

    #[test]
    fn mock_with_select_is_a_read() {
        let log = line("oo_reader", "SELECT 1")
            + &line("oo_reader", "WITH s AS (SELECT 1) SELECT * FROM s");
        let report = classify_csv(&log, "oo_reader");
        assert!(report.zero_write.proven);
        assert_eq!(report.forbidden, 0);
        assert!(report.reader_verbs.contains(&"WITH".to_string()));
    }

    #[test]
    fn mock_with_insert_is_a_write() {
        let log = line(
            "oo_reader",
            "WITH s AS (INSERT INTO app.item VALUES (1, 'x') RETURNING id) SELECT * FROM s",
        );
        let report = classify_csv(&log, "oo_reader");
        assert!(!report.zero_write.proven);
        assert_eq!(report.forbidden, 1);
        assert_eq!(report.zero_write.basis, "audit_write");
    }

    #[test]
    fn mock_reader_insert_is_a_write() {
        let report = classify_csv(
            &line("oo_reader", "INSERT INTO app.item VALUES (1, 'x')"),
            "oo_reader",
        );
        assert!(!report.zero_write.proven);
        assert_eq!(report.forbidden, 1);
        assert_eq!(report.zero_write.basis, "audit_write");
    }
}
