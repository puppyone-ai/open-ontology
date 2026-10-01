use crate::catalog::{self, Col, GJ, Table};
use crate::error::{Result, err};
use crate::plan::shard_list;

pub fn mysql_ddl(integer_fen: bool) -> String {
    let mut out = String::from(
        "-- oo-logistics/v1 synthetic MySQL ERP DDL\n-- No FOREIGN KEY. Comments are not applied from this file.\n",
    );
    for table in crate::catalog::mysql_tables() {
        out.push_str(&create_mysql(table, integer_fen));
        out.push('\n');
    }
    out
}

pub fn postgres_ddl() -> String {
    postgres_ddl_for(shard_list(), true)
}

pub fn postgres_ddl_for(shards: &[u32], integer_fen: bool) -> String {
    let mut out = String::from(
        "-- oo-logistics/v1 synthetic PostgreSQL TMS DDL\n-- No FOREIGN KEY. Monthly gj shards have no primary key.\n",
    );
    for table in crate::catalog::postgres_tables() {
        if table.name == "gj" {
            for shard in shards {
                out.push_str(&create_pg_named(
                    &format!("gj_{shard}"),
                    GJ.columns,
                    &[],
                    "postgres",
                    integer_fen,
                ));
                out.push('\n');
            }
        } else {
            out.push_str(&create_pg(table, integer_fen));
            out.push('\n');
        }
    }
    out
}

fn create_mysql(table: &Table, integer_fen: bool) -> String {
    let mut out = format!("CREATE TABLE `{}` (\n", table.name);
    for col in table.columns {
        let sql = catalog::effective_sql("mysql", col, integer_fen);
        out.push_str(&format!("  `{}` {}{},\n", col.name, sql, null_sql(col)));
    }
    if table.pk.is_empty() {
        return create_mysql_no_pk(table, integer_fen);
    }
    let pk = table
        .pk
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "  PRIMARY KEY ({pk})\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;\n"
    ));
    out
}

fn create_mysql_no_pk(table: &Table, integer_fen: bool) -> String {
    let mut out = format!("CREATE TABLE `{}` (\n", table.name);
    for (index, col) in table.columns.iter().enumerate() {
        let comma = if index + 1 == table.columns.len() {
            ""
        } else {
            ","
        };
        let sql = catalog::effective_sql("mysql", col, integer_fen);
        out.push_str(&format!(
            "  `{}` {}{}{comma}\n",
            col.name, sql, null_sql(col)
        ));
    }
    out.push_str(") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;\n");
    out
}

fn create_pg(table: &Table, integer_fen: bool) -> String {
    create_pg_named(table.name, table.columns, table.pk, "postgres", integer_fen)
}

fn create_pg_named(
    name: &str,
    columns: &[Col],
    pk: &[&str],
    dialect: &str,
    integer_fen: bool,
) -> String {
    let mut out = format!("CREATE TABLE \"{name}\" (\n");
    for (index, col) in columns.iter().enumerate() {
        let last = index + 1 == columns.len() && pk.is_empty();
        let comma = if last { "" } else { "," };
        let sql = catalog::effective_sql(dialect, col, integer_fen);
        out.push_str(&format!(
            "  \"{}\" {}{}{comma}\n",
            col.name, sql, null_sql(col)
        ));
    }
    if !pk.is_empty() {
        let cols = pk
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("  PRIMARY KEY ({cols})\n"));
    }
    out.push_str(");\n");
    out
}

fn null_sql(col: &Col) -> &'static str {
    if col.nullable { "" } else { " NOT NULL" }
}

pub fn ddl_has_foreign_key(sql: &str) -> bool {
    let upper = strip_sql_comments(sql).to_ascii_uppercase();
    upper.contains("FOREIGN KEY") || upper.contains("REFERENCES")
}

/// Line and block comments are not constraints. A note that says "No FOREIGN KEY"
/// must not count as a key.
fn strip_sql_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let bytes = sql.as_bytes();
    let mut index = 0;
    let mut quote = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte >= 128 {
            let ch = sql[index..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            index += ch.len_utf8();
            continue;
        }
        if let Some(mark) = quote {
            out.push(byte as char);
            if byte == mark {
                if bytes.get(index + 1) == Some(&mark) {
                    out.push(mark as char);
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
            out.push(byte as char);
            index += 1;
            continue;
        }
        if byte == b'-' && bytes.get(index + 1) == Some(&b'-') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            out.push(' ');
            continue;
        }
        out.push(byte as char);
        index += 1;
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedColumn {
    pub name: String,
    pub sql: String,
    pub nullable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedTable {
    pub name: String,
    pub columns: Vec<ParsedColumn>,
    pub pk: Vec<String>,
}

/// Limited scanner for the DDL this generator writes. It is not a SQL parser.
/// Comments are removed first. Commas split columns only at parenthesis depth 0,
/// so `enum('a','b')` and `decimal(40,12)` stay one type.
pub fn parse_creates(sql: &str) -> Result<Vec<ParsedTable>> {
    let stripped = strip_sql_comments(sql);
    let mut tables = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for statement in split_statements(&stripped)? {
        let table = parse_create(statement)?;
        if !seen.insert(table.name.clone()) {
            return Err(err(format!("duplicate create table {}", table.name)));
        }
        tables.push(table);
    }
    Ok(tables)
}

fn split_statements(sql: &str) -> Result<Vec<&str>> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut quote = None;
    let mut depth = 0i32;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte >= 128 {
            index += sql[index..].chars().next().map_or(1, |ch| ch.len_utf8());
            continue;
        }
        if let Some(mark) = quote {
            if byte == mark {
                if bytes.get(index + 1) == Some(&mark) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth -= 1;
            if depth < 0 {
                return Err(err("ddl statement is not a create table"));
            }
        } else if byte == b';' && depth == 0 {
            let statement = sql[start..index].trim();
            if !statement.is_empty() {
                out.push(statement);
            }
            start = index + 1;
        }
        index += 1;
    }
    if quote.is_some() || depth != 0 || !sql[start..].trim().is_empty() {
        return Err(err("ddl statement is not a create table"));
    }
    Ok(out)
}

fn parse_create(statement: &str) -> Result<ParsedTable> {
    let rest = statement
        .strip_prefix("CREATE TABLE ")
        .ok_or_else(|| err("ddl statement is not a create table"))?;
    let bytes = rest.as_bytes();
    if bytes.is_empty() || (bytes[0] != b'`' && bytes[0] != b'"') {
        return Err(err("ddl statement is not a create table"));
    }
    let mark = bytes[0];
    let mut index = 1;
    while index < bytes.len() && bytes[index] != mark {
        if bytes[index] >= 128 {
            index += rest[index..].chars().next().map_or(1, |ch| ch.len_utf8());
            continue;
        }
        index += 1;
    }
    if index >= bytes.len() {
        return Err(err("ddl statement is not a create table"));
    }
    let name = &rest[1..index];
    if name.is_empty() {
        return Err(err("ddl statement is not a create table"));
    }
    index += 1;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index) != Some(&b'(') {
        return Err(err("ddl statement is not a create table"));
    }
    let body_start = index + 1;
    let mut depth = 1i32;
    let mut quote = None;
    let mut end = body_start;
    while end < bytes.len() {
        let byte = bytes[end];
        if byte >= 128 {
            end += rest[end..].chars().next().map_or(1, |ch| ch.len_utf8());
            continue;
        }
        if let Some(mark) = quote {
            if byte == mark {
                if bytes.get(end + 1) == Some(&mark) {
                    end += 2;
                    continue;
                }
                quote = None;
            }
            end += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
            end += 1;
            continue;
        }
        if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        end += 1;
    }
    if end >= bytes.len() || bytes[end] != b')' {
        return Err(err("ddl statement is not a create table"));
    }
    let body = &rest[body_start..end];
    let tail = &rest[end + 1..];
    if ddl_has_foreign_key(tail) {
        return Err(err("ddl contains a foreign key"));
    }
    parse_body(name, body)
}

fn parse_body(table: &str, body: &str) -> Result<ParsedTable> {
    let mut columns = Vec::new();
    let mut pk = Vec::new();
    let mut saw_pk = false;
    for item in split_items(body)? {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let upper = item.to_ascii_uppercase();
        if upper.contains("FOREIGN KEY") || upper.contains("REFERENCES") {
            return Err(err("ddl contains a foreign key"));
        }
        if let Some(rest) = item.strip_prefix("PRIMARY KEY") {
            if saw_pk {
                return Err(err(format!("primary key drift on {table}")));
            }
            saw_pk = true;
            pk = parse_pk(table, rest)?;
            continue;
        }
        columns.push(parse_column(table, item)?);
    }
    Ok(ParsedTable {
        name: table.to_string(),
        columns,
        pk,
    })
}

fn split_items(body: &str) -> Result<Vec<&str>> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut quote = None;
    let mut depth = 0i32;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte >= 128 {
            index += body[index..].chars().next().map_or(1, |ch| ch.len_utf8());
            continue;
        }
        if let Some(mark) = quote {
            if byte == mark {
                if bytes.get(index + 1) == Some(&mark) {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth -= 1;
            if depth < 0 {
                return Err(err("ddl statement is not a create table"));
            }
        } else if byte == b',' && depth == 0 {
            out.push(&body[start..index]);
            start = index + 1;
        }
        index += 1;
    }
    if quote.is_some() || depth != 0 {
        return Err(err("ddl statement is not a create table"));
    }
    out.push(&body[start..]);
    Ok(out)
}

fn parse_pk(table: &str, rest: &str) -> Result<Vec<String>> {
    let rest = rest.trim();
    if !rest.starts_with('(') || !rest.ends_with(')') {
        return Err(err(format!("primary key name is unquoted on {table}")));
    }
    let inner = &rest[1..rest.len() - 1];
    let mut names = Vec::new();
    for part in split_items(inner)? {
        let part = part.trim();
        if part.is_empty() {
            return Err(err(format!("primary key drift on {table}")));
        }
        names.push(quoted_name(table, part)?);
    }
    if names.is_empty() {
        return Err(err(format!("primary key drift on {table}")));
    }
    Ok(names)
}

fn quoted_name(table: &str, text: &str) -> Result<String> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 || (bytes[0] != b'`' && bytes[0] != b'"') || bytes[bytes.len() - 1] != bytes[0]
    {
        return Err(err(format!("primary key name is unquoted on {table}")));
    }
    let name = &text[1..text.len() - 1];
    if name.is_empty() || name.as_bytes().contains(&bytes[0]) {
        return Err(err(format!("primary key name is unquoted on {table}")));
    }
    Ok(name.to_string())
}

fn parse_column(table: &str, item: &str) -> Result<ParsedColumn> {
    let bytes = item.as_bytes();
    if bytes.is_empty() || (bytes[0] != b'`' && bytes[0] != b'"') {
        return Err(err(format!("ddl column name is unquoted on {table}")));
    }
    let mark = bytes[0];
    let mut index = 1;
    while index < bytes.len() && bytes[index] != mark {
        if bytes[index] >= 128 {
            index += item[index..].chars().next().map_or(1, |ch| ch.len_utf8());
            continue;
        }
        index += 1;
    }
    if index >= bytes.len() {
        return Err(err(format!("ddl column name is unquoted on {table}")));
    }
    let name = &item[1..index];
    if name.is_empty() {
        return Err(err(format!("ddl column name is unquoted on {table}")));
    }
    let rest = item[index + 1..].trim();
    let (sql, nullable) = if let Some(sql) = rest.strip_suffix(" NOT NULL") {
        (sql.trim(), false)
    } else {
        (rest, true)
    };
    if sql.is_empty() {
        return Err(err(format!("ddl type {table}.{name} is , expected a type")));
    }
    Ok(ParsedColumn {
        name: name.to_string(),
        sql: sql.to_string(),
        nullable,
    })
}

#[cfg(test)]
mod tests {
    use super::{ddl_has_foreign_key, mysql_ddl, parse_creates, postgres_ddl_for};

    #[test]
    fn comments_are_not_foreign_keys() {
        assert!(!ddl_has_foreign_key(
            "-- No FOREIGN KEY.\nCREATE TABLE `kh` (`khid` varchar(32) NOT NULL, PRIMARY KEY (`khid`));\n"
        ));
        assert!(ddl_has_foreign_key(
            "CREATE TABLE t (a int, FOREIGN KEY (a) REFERENCES u(a));"
        ));
        assert!(ddl_has_foreign_key(
            "CREATE TABLE t (a int REFERENCES u(a));"
        ));
        assert!(!ddl_has_foreign_key(
            "/* REFERENCES nothing */\nCREATE TABLE t (a int);\n"
        ));
    }

    #[test]
    fn cents_off_changes_money_types_only() {
        let on = mysql_ddl(true);
        let off = mysql_ddl(false);
        assert!(on.contains("`je_fen` bigint NOT NULL"));
        assert!(off.contains("`je_fen` decimal(20,2) NOT NULL"));
        assert!(on.contains("`hlje` decimal(40,12)"));
        assert!(off.contains("`hlje` decimal(40,12)"));
        assert!(!on.contains("decimal(20,2)"));
        let pg_on = postgres_ddl_for(&[202601], true);
        let pg_off = postgres_ddl_for(&[202601], false);
        assert!(pg_on.contains("\"sfje\" bigint NOT NULL"));
        assert!(pg_off.contains("\"sfje\" numeric(20,2) NOT NULL"));
        assert!(pg_off.contains("\"sl\" integer NOT NULL"));
        assert!(pg_on.contains("\"je_fen\" bigint NOT NULL"));
        assert!(pg_off.contains("\"je_fen\" numeric(20,2) NOT NULL"));
    }

    #[test]
    fn parser_scopes_keys_types_and_enum_commas() {
        let parsed = parse_creates(&mysql_ddl(true)).unwrap();
        let kh = parsed.iter().find(|table| table.name == "kh").unwrap();
        assert_eq!(kh.pk, vec!["khid".to_string()]);
        assert_eq!(kh.columns[0].sql, "varchar(32)");
        assert!(!kh.columns[0].nullable);
        let enumerated = kh
            .columns
            .iter()
            .find(|column| column.name == "khzt_enum")
            .unwrap();
        assert_eq!(enumerated.sql, "enum('0','1','2')");
        let tags = kh
            .columns
            .iter()
            .find(|column| column.name == "khbq")
            .unwrap();
        assert_eq!(tags.sql, "set('vip','delay','trial')");
        let rate = kh
            .columns
            .iter()
            .find(|column| column.name == "hlje")
            .unwrap();
        assert_eq!(rate.sql, "decimal(40,12)");
        assert!(rate.nullable);

        let spoof = parse_creates(
            "-- PRIMARY KEY (`khid`)\nCREATE TABLE `kh` (\n  `khid` varchar(32) NOT NULL\n);\n",
        )
        .unwrap();
        assert!(spoof[0].pk.is_empty());

        let other = parse_creates(
            "CREATE TABLE `other` (\n  `khid` varchar(32) NOT NULL,\n  PRIMARY KEY (`khid`)\n);\nCREATE TABLE `kh` (\n  `khid` varchar(1) NOT NULL\n);\n",
        )
        .unwrap();
        assert_eq!(other[0].pk, vec!["khid".to_string()]);
        assert!(other[1].pk.is_empty());
        assert_eq!(other[1].columns[0].sql, "varchar(1)");

        let unquoted = parse_creates(
            "CREATE TABLE `kh` (\n  `khid` varchar(32) NOT NULL,\n  PRIMARY KEY (khid)\n);\n",
        )
        .unwrap_err();
        assert!(unquoted.to_string().contains("unquoted"));

        let extra = parse_creates("SELECT 1;\n").unwrap_err();
        assert!(
            extra
                .to_string()
                .contains("ddl statement is not a create table")
        );

        let duplicate = parse_creates(
            "CREATE TABLE `kh` (\n  `khid` varchar(32) NOT NULL\n);\nCREATE TABLE `kh` (\n  `khid` varchar(32) NOT NULL\n);\n",
        )
        .unwrap_err();
        assert!(duplicate.to_string().contains("duplicate create table kh"));

        let shards = parse_creates(&postgres_ddl_for(&[202601, 202611], true)).unwrap();
        let shard = shards
            .iter()
            .find(|table| table.name == "gj_202601")
            .unwrap();
        assert!(shard.pk.is_empty());
        assert!(shards.iter().any(|table| table.name == "gj_202611"));
    }
}
