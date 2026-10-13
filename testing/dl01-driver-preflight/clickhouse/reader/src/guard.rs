use crate::fail::{fail, Gate};

const ALLOWED: &[&str] = &["select", "show", "exists", "describe", "desc", "with"];

const FORBIDDEN: &[&str] = &[
    "insert", "delete", "alter", "create", "drop", "truncate", "optimize", "kill", "grant",
    "revoke", "attach", "detach", "rename", "exchange", "system", "update", "replace", "set",
];

pub fn assert_reader_sql(sql: &str) -> Gate<()> {
    let stripped = strip_comments(sql);
    let keyword = first_keyword(&stripped);
    if keyword.is_empty() {
        return Err(fail("reader statement is empty"));
    }
    if FORBIDDEN.contains(&keyword.as_str()) {
        return Err(fail(format!("reader statement rejected locally: {keyword}")));
    }
    if !ALLOWED.contains(&keyword.as_str()) {
        return Err(fail(format!("reader statement kind is not a read: {keyword}")));
    }
    if extra_semicolon(&stripped) {
        return Err(fail("reader statement has more than one statement"));
    }
    Ok(())
}

fn strip_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '-' && chars.peek() == Some(&'-') {
            chars.next();
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut previous = '\0';
            for next in chars.by_ref() {
                if previous == '*' && next == '/' {
                    break;
                }
                previous = next;
            }
            out.push(' ');
            continue;
        }
        out.push(ch);
    }
    out
}

fn first_keyword(sql: &str) -> String {
    let mut word = String::new();
    for ch in sql.chars() {
        if ch.is_ascii_alphabetic() {
            word.push(ch.to_ascii_lowercase());
        } else if !word.is_empty() || !ch.is_whitespace() {
            break;
        }
    }
    word
}

fn extra_semicolon(sql: &str) -> bool {
    let trimmed = sql.trim().trim_end_matches(';').trim_end();
    trimmed.contains(';')
}

#[cfg(test)]
mod tests {
    use super::assert_reader_sql;

    #[test]
    fn select_is_a_read() {
        assert_reader_sql("SELECT 1 FORMAT JSONEachRow").unwrap();
    }

    #[test]
    fn settings_clause_stays_a_select() {
        assert_reader_sql("SELECT sleep(30) SETTINGS max_execution_time = 2").unwrap();
    }

    #[test]
    fn kill_insert_and_set_are_local_rejects() {
        for sql in ["KILL QUERY WHERE query_id = 'x'", "INSERT INTO t VALUES (1)", "SET readonly = 0"] {
            let error = assert_reader_sql(sql).unwrap_err();
            assert!(error.0.contains("rejected locally"), "{error}");
        }
    }

    #[test]
    fn two_statements_are_rejected() {
        let error = assert_reader_sql("SELECT 1; SELECT 2").unwrap_err();
        assert!(error.0.contains("more than one"));
    }
}
