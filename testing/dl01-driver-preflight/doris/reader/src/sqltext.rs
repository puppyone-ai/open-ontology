pub fn split(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    for line in sql.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        current.push_str(trimmed);
        current.push('\n');
        if trimmed.ends_with(';') {
            push_statement(&mut statements, &mut current);
        }
    }
    if !current.trim().is_empty() {
        push_statement(&mut statements, &mut current);
    }
    statements
}

fn push_statement(statements: &mut Vec<String>, current: &mut String) {
    let text = current.trim().trim_end_matches(';').trim().to_string();
    current.clear();
    if !text.is_empty() {
        statements.push(text);
    }
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn skips_comments_and_keeps_statements() {
        let sql = "-- note\nUSE oo_dl01w1_43;\nSELECT 1;\n";
        assert_eq!(split(sql), vec!["USE oo_dl01w1_43".to_string(), "SELECT 1".to_string()]);
    }
}
