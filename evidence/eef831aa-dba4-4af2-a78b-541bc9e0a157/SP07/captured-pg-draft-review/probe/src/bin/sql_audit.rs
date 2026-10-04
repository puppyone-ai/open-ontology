//! Classify PostgreSQL `statement:` and extended-protocol `execute` lines.
//!
//! Passwords and the fixture comment are read from files. They are not arguments.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

const READER: &str = "oo_a157_reader";
const ADMIN: &str = "postgres";

const DATA_WRITES: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "TRUNCATE", "ALTER", "CREATE", "DROP", "GRANT", "REVOKE",
    "COPY", "COMMENT", "VACUUM", "ANALYZE", "CLUSTER", "REINDEX", "LOCK", "CALL", "DO",
    "REFRESH", "IMPORT", "MERGE", "CHECKPOINT", "SECURITY",
];

const READS: &[&str] = &[
    "SELECT", "WITH", "TABLE", "VALUES", "EXPLAIN", "SHOW", "SET", "RESET", "BEGIN", "START",
    "ROLLBACK", "SAVEPOINT", "RELEASE", "PREPARE", "EXECUTE", "DEALLOCATE", "FETCH", "CLOSE",
    "DISCARD", "LISTEN", "UNLISTEN", "DECLARE",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Simple,
    Execute,
}

struct Statement {
    user: String,
    protocol: Protocol,
    body: String,
}

struct Summary {
    reader_statements: usize,
    reader_simple: usize,
    reader_execute: usize,
    reader_data_writes: usize,
    reader_unclassified: usize,
    reader_keywords: BTreeMap<String, usize>,
    admin_keywords: BTreeMap<String, usize>,
    reader_write_keywords: Vec<String>,
    reader_unclassified_keywords: Vec<String>,
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    let result = match mode.as_str() {
        "self-check" => self_check(),
        "classify" => classify_args(args.collect()),
        _ => Err("usage: sql_audit self-check | classify --log PATH --secrets DIR --out DIR --comment-file PATH".to_string()),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            let _ = writeln!(io::stderr(), "sql_audit_error={err}");
            ExitCode::from(1)
        }
    }
}

fn classify_args(args: Vec<String>) -> Result<u8, String> {
    let mut log = None;
    let mut secrets = None;
    let mut out = None;
    let mut comment_file = None;
    let mut iter = args.into_iter();
    while let Some(flag) = iter.next() {
        let value = iter.next().ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--log" => log = Some(value),
            "--secrets" => secrets = Some(value),
            "--out" => out = Some(value),
            "--comment-file" => comment_file = Some(value),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let log = log.ok_or("--log")?;
    let secrets = secrets.ok_or("--secrets")?;
    let out = out.ok_or("--out")?;
    let comment_file = comment_file.ok_or("--comment-file")?;
    fs::create_dir_all(&out).map_err(|err| err.to_string())?;
    let mut text = fs::read_to_string(&log).map_err(|err| err.to_string())?;
    for name in ["super.pass", "reader.pass"] {
        let raw = fs::read_to_string(Path::new(&secrets).join(name)).unwrap_or_default();
        let secret = raw.trim();
        if !secret.is_empty() {
            text = text.replace(secret, "[redacted]");
        }
    }
    let comment = fs::read_to_string(&comment_file).unwrap_or_default();
    let comment = comment.trim();
    if !comment.is_empty() {
        text = text.replace(comment, "[redacted-comment]");
    }
    let summary = summarize(&text);
    let rendered = render(&summary);
    fs::write(Path::new(&out).join("audit.txt"), &rendered).map_err(|err| err.to_string())?;
    fs::write(Path::new(&out).join("postgresql.redacted.log"), &text).map_err(|err| err.to_string())?;
    let code = if summary.reader_data_writes > 0 || summary.reader_unclassified > 0 {
        1
    } else if summary.reader_execute == 0 {
        2
    } else {
        0
    };
    println!("{rendered}");
    println!("classify_exit={code}");
    Ok(code)
}

fn self_check() -> Result<u8, String> {
    let fixture = "\
2026-10-04 00:00:00.000 UTC pid=1 app= user=oo_a157_reader db=oo_a157 LOG:  execute sqlx_s_1: SELECT 1
2026-10-04 00:00:00.001 UTC pid=1 app= user=oo_a157_reader db=oo_a157 LOG:  statement: START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY
2026-10-04 00:00:00.002 UTC pid=1 app= user=oo_a157_reader db=oo_a157 LOG:  statement: INSERT INTO ledger.party VALUES (1)
2026-10-04 00:00:00.003 UTC pid=1 app= user=postgres db=oo_a157 LOG:  statement: ALTER TABLE ledger.party ADD COLUMN note text
2026-10-04 00:00:00.004 UTC pid=1 app= user=oo_a157_reader db=oo_a157 LOG:  could not receive data from client
";
    let summary = summarize(fixture);
    if summary.reader_execute != 1 {
        return Err(format!("self-check ignored extended protocol: execute={}", summary.reader_execute));
    }
    if summary.reader_simple != 2 {
        return Err(format!("self-check simple count {}", summary.reader_simple));
    }
    if summary.reader_statements != 3 {
        return Err(format!("self-check counted a non-statement: {}", summary.reader_statements));
    }
    if summary.reader_data_writes != 1 || summary.reader_write_keywords != ["INSERT"] {
        return Err("self-check missed INSERT".to_string());
    }
    if summary.admin_keywords.get("ALTER") != Some(&1) {
        return Err("self-check missed admin ALTER".to_string());
    }
    if summary.reader_keywords.get("SELECT") != Some(&1) || summary.reader_keywords.get("START") != Some(&1) {
        return Err("self-check keyword map is wrong".to_string());
    }
    println!("self_check_ok reader_execute=1 reader_simple=2 insert_seen=1 disconnect_ignored=1");
    Ok(0)
}

fn summarize(text: &str) -> Summary {
    let mut summary = Summary {
        reader_statements: 0,
        reader_simple: 0,
        reader_execute: 0,
        reader_data_writes: 0,
        reader_unclassified: 0,
        reader_keywords: BTreeMap::new(),
        admin_keywords: BTreeMap::new(),
        reader_write_keywords: Vec::new(),
        reader_unclassified_keywords: Vec::new(),
    };
    for item in statements(text) {
        for command in commands(&item.body) {
            let word = keyword(&command);
            if word.is_empty() {
                continue;
            }
            if item.user == READER {
                summary.reader_statements += 1;
                match item.protocol {
                    Protocol::Simple => summary.reader_simple += 1,
                    Protocol::Execute => summary.reader_execute += 1,
                }
                *summary.reader_keywords.entry(word.clone()).or_insert(0) += 1;
                if DATA_WRITES.contains(&word.as_str()) {
                    summary.reader_data_writes += 1;
                    summary.reader_write_keywords.push(word);
                } else if !READS.contains(&word.as_str()) {
                    summary.reader_unclassified += 1;
                    summary.reader_unclassified_keywords.push(word);
                }
            } else if item.user == ADMIN {
                *summary.admin_keywords.entry(word).or_insert(0) += 1;
            }
        }
    }
    summary
}

fn render(summary: &Summary) -> String {
    let mut lines = vec![
        format!("reader_statements={}", summary.reader_statements),
        format!("reader_simple={}", summary.reader_simple),
        format!("reader_execute={}", summary.reader_execute),
        format!("reader_data_writes={}", summary.reader_data_writes),
        format!("reader_unclassified={}", summary.reader_unclassified),
        "reader_keywords:".to_string(),
    ];
    for (word, count) in &summary.reader_keywords {
        lines.push(format!("  {word}={count}"));
    }
    lines.push("admin_keywords:".to_string());
    for (word, count) in &summary.admin_keywords {
        lines.push(format!("  {word}={count}"));
    }
    if !summary.reader_write_keywords.is_empty() {
        lines.push(format!("reader_write_keywords={}", summary.reader_write_keywords.join(",")));
    }
    if !summary.reader_unclassified_keywords.is_empty() {
        lines.push(format!(
            "reader_unclassified_keywords={}",
            summary.reader_unclassified_keywords.join(",")
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn statements(text: &str) -> Vec<Statement> {
    let mut out = Vec::new();
    let mut current: Option<Statement> = None;
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        if let Some((user, rest)) = split_head(line) {
            if let Some(done) = current.take() {
                out.push(done);
            }
            if let Some((protocol, body)) = statement_body(&rest) {
                current = Some(Statement { user, protocol, body });
            }
            continue;
        }
        if let Some(item) = current.as_mut() {
            item.body.push('\n');
            item.body.push_str(line);
        }
    }
    if let Some(done) = current {
        out.push(done);
    }
    out
}

fn split_head(line: &str) -> Option<(String, String)> {
    let pid = line.find(" pid=")?;
    let after_pid = &line[pid + " pid=".len()..];
    let mut index = 0;
    let bytes = after_pid.as_bytes();
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 0 {
        return None;
    }
    let after = &after_pid[index..];
    let app = after.strip_prefix(" app=")?;
    let user_at = app.find(" user=")?;
    let after_user = &app[user_at + " user=".len()..];
    let db_at = after_user.find(" db=")?;
    let user = after_user[..db_at].to_string();
    let after_db = &after_user[db_at + " db=".len()..];
    let space = after_db.find(' ')?;
    let after_level = &after_db[space + 1..];
    let colon = after_level.find(':')?;
    let rest = after_level[colon + 1..].trim_start().to_string();
    Some((user, rest))
}

fn statement_body(rest: &str) -> Option<(Protocol, String)> {
    if let Some(body) = rest.strip_prefix("statement:") {
        return Some((Protocol::Simple, body.trim_start().to_string()));
    }
    let after = rest.strip_prefix("execute ")?;
    let colon = after.find(':')?;
    let name = &after[..colon];
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return None;
    }
    Some((Protocol::Execute, after[colon + 1..].trim_start().to_string()))
}

fn commands(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = sql.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '-' && chars.get(index + 1) == Some(&'-') {
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if ch == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                index += 1;
            }
            index = (index + 2).min(chars.len());
            continue;
        }
        if ch == '\'' {
            current.push(ch);
            index += 1;
            while index < chars.len() {
                current.push(chars[index]);
                if chars[index] == '\'' {
                    if chars.get(index + 1) == Some(&'\'') {
                        current.push('\'');
                        index += 2;
                        continue;
                    }
                    index += 1;
                    break;
                }
                index += 1;
            }
            continue;
        }
        if ch == '$' {
            let mut end = index + 1;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            if chars.get(end) == Some(&'$') {
                let tag: String = chars[index..=end].iter().collect();
                current.push_str(&tag);
                index = end + 1;
                while index < chars.len() && !chars[index..].starts_with(tag.chars().collect::<Vec<_>>().as_slice()) {
                    current.push(chars[index]);
                    index += 1;
                }
                if index < chars.len() {
                    current.push_str(&tag);
                    index += tag.chars().count();
                }
                continue;
            }
        }
        if ch == ';' {
            if !current.trim().is_empty() {
                out.push(current.clone());
            }
            current.clear();
            index += 1;
            continue;
        }
        current.push(ch);
        index += 1;
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

fn keyword(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut index = 0;
    loop {
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index + 1 < chars.len() && chars[index] == '-' && chars[index + 1] == '-' {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if index + 1 < chars.len() && chars[index] == '/' && chars[index + 1] == '*' {
            index += 2;
            while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                index += 1;
            }
            index = (index + 2).min(chars.len());
            continue;
        }
        break;
    }
    if index >= chars.len() {
        return String::new();
    }
    let mut token = String::new();
    while index < chars.len() && !chars[index].is_whitespace() && chars[index] != '(' && chars[index] != ';' {
        token.push(chars[index]);
        index += 1;
    }
    token.to_ascii_uppercase()
}
