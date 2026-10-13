use crate::fail::Fail;

pub fn require_mariadb(version: &str, comment: &str) -> Result<(), Fail> {
    let comment_lower = comment.to_ascii_lowercase();
    if !version.contains("MariaDB") || !comment_lower.contains("mariadb") {
        return Err(Fail::new(
            "not_mariadb",
            format!("version={version} comment={comment}"),
        ));
    }
    if !version.starts_with("11.4.13-MariaDB") {
        return Err(Fail::new("service_version", version.to_string()));
    }
    Ok(())
}

pub fn require_fixture_session(sql_mode: &str, time_zone: &str) -> Result<(), Fail> {
    for needle in ["NO_ZERO_DATE", "NO_ZERO_IN_DATE", "STRICT_TRANS_TABLES"] {
        if !sql_mode.split(',').any(|part| part == needle) {
            return Err(Fail::new("sql_mode", format!("missing {needle} in {sql_mode}")));
        }
    }
    if time_zone != "+00:00" {
        return Err(Fail::new("time_zone", time_zone.to_string()));
    }
    Ok(())
}

pub fn grant_is_reader_only(line: &str) -> bool {
    let kept = line.split(" IDENTIFIED").next().unwrap_or(line);
    let upper = kept.to_ascii_uppercase();
    if upper.contains("USAGE") && !upper.contains("SELECT") {
        return true;
    }
    const DENIED: &[&str] = &[
        "INSERT",
        "UPDATE",
        "DELETE",
        "CREATE",
        "DROP",
        "ALTER",
        "PROCESS",
        "CONNECTION ADMIN",
        "ALL PRIVILEGES",
        "SUPER",
        "FILE",
        "GRANT OPTION",
        "RELOAD",
        "SHUTDOWN",
        "BINLOG",
    ];
    if DENIED.iter().any(|word| upper.contains(word)) {
        return false;
    }
    if upper.contains("SELECT") && upper.contains("*.*") {
        return false;
    }
    upper.contains("SELECT") && upper.contains("OO_SYNTH")
}

pub fn redact_grant(line: &str) -> String {
    line.split(" IDENTIFIED").next().unwrap_or(line).trim().to_string()
}

pub fn grants_ok(lines: &[String]) -> Result<(), Fail> {
    if lines.is_empty() {
        return Err(Fail::new("grants", "empty"));
    }
    let mut saw_select = false;
    for line in lines {
        if !grant_is_reader_only(line) {
            return Err(Fail::new("grants", redact_grant(line)));
        }
        let upper = line.split(" IDENTIFIED").next().unwrap_or(line).to_ascii_uppercase();
        if upper.contains("SELECT") && upper.contains("OO_SYNTH") {
            saw_select = true;
        }
    }
    if !saw_select {
        return Err(Fail::new("grants", "missing SELECT on oo_synth"));
    }
    Ok(())
}
