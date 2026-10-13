//! Fixed readonly statements. The DBA negative-write script is not a customer reader.

use crate::fail::Fail;

pub const SERVER_SQL: &str = "SELECT CAST(SERVERPROPERTY(N'ProductVersion') AS nvarchar(128)) AS product_version, CAST(SERVERPROPERTY(N'ProductLevel') AS nvarchar(128)) AS product_level, CAST(SERVERPROPERTY(N'Edition') AS nvarchar(128)) AS edition, CAST(SERVERPROPERTY(N'Collation') AS nvarchar(128)) AS server_collation, DB_NAME() AS database_name, SUSER_SNAME() AS login_name";

pub const CATALOG_SQL: &str = "SELECT s.name AS schema_name, t.name AS table_name, c.column_id AS column_id, c.name AS column_name, ty.name AS type_name, c.max_length AS max_length, c.precision AS precision, c.scale AS scale, c.is_nullable AS is_nullable, c.is_identity AS is_identity, c.collation_name AS collation_name FROM sys.tables AS t INNER JOIN sys.schemas AS s ON s.schema_id = t.schema_id INNER JOIN sys.columns AS c ON c.object_id = t.object_id INNER JOIN sys.types AS ty ON ty.user_type_id = c.user_type_id WHERE s.name = @P1 AND t.name = @P2 ORDER BY c.column_id";

pub const PERMISSION_SQL: &str = "SELECT HAS_PERMS_BY_NAME(@P1, N'OBJECT', N'SELECT') AS can_select, HAS_PERMS_BY_NAME(@P1, N'OBJECT', N'INSERT') AS can_insert, HAS_PERMS_BY_NAME(@P1, N'OBJECT', N'UPDATE') AS can_update, HAS_PERMS_BY_NAME(@P1, N'OBJECT', N'DELETE') AS can_delete, HAS_PERMS_BY_NAME(@P1, N'OBJECT', N'ALTER') AS can_alter";

const WRITE_WORDS: &[&str] = &[
    "insert",
    "update",
    "delete",
    "merge",
    "truncate",
    "create",
    "alter",
    "drop",
    "exec",
    "execute",
    "grant",
    "revoke",
    "deny",
    "backup",
    "restore",
    "kill",
    "waitfor",
    "dbcc",
    "bulk",
    "into",
    "openrowset",
    "opendatasource",
    "changetable",
    "xp_cmdshell",
];

pub fn load_sql(schema: &str, name: &str, columns: &[String], limit: u64) -> Result<String, Fail> {
    require_ident(schema)?;
    require_ident(name)?;
    if columns.is_empty() || limit == 0 || limit > 1_000_000_000 {
        return Err(Fail::new("load_sql", "column list or row cap is empty"));
    }
    let mut projected = String::new();
    for (index, column) in columns.iter().enumerate() {
        require_ident(column)?;
        if index > 0 {
            projected.push_str(", ");
        }
        projected.push('[');
        projected.push_str(column);
        projected.push(']');
    }
    Ok(format!(
        "SELECT TOP ({limit}) {projected} FROM [{schema}].[{name}]"
    ))
}

pub fn admit_product_sql(sql: &str) -> Result<(), Fail> {
    if sql == SERVER_SQL || sql == CATALOG_SQL || sql == PERMISSION_SQL || parse_load_sql(sql).is_ok() {
        return Ok(());
    }
    let lower = sql.trim().to_ascii_lowercase();
    let mut word = String::new();
    let mut words = Vec::new();
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            word.push(ch);
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    for found in &words {
        if WRITE_WORDS.contains(&found.as_str()) || found.starts_with("sp_cdc") || found.starts_with("xp_") {
            return Err(Fail::new("write_sql", "statement is not a customer read"));
        }
    }
    Err(Fail::new(
        "not_product_sql",
        "statement is outside the fixed customer SELECT set",
    ))
}

fn parse_load_sql(sql: &str) -> Result<(), Fail> {
    let Some(rest) = sql.strip_prefix("SELECT TOP (") else {
        return Err(Fail::new("not_product_sql", "not a bounded load"));
    };
    let Some((limit, after_limit)) = rest.split_once(") ") else {
        return Err(Fail::new("not_product_sql", "not a bounded load"));
    };
    if limit.is_empty() || !limit.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Fail::new("not_product_sql", "row cap is not a decimal"));
    }
    let Some((projection, relation)) = after_limit.split_once(" FROM ") else {
        return Err(Fail::new("not_product_sql", "not a bounded load"));
    };
    if relation.contains(';') || projection.contains(';') || sql.contains("--") || sql.contains("/*") {
        return Err(Fail::new("write_sql", "statement is not a customer read"));
    }
    let Some((schema, name)) = relation.split_once('.') else {
        return Err(Fail::new("not_product_sql", "load relation is not schema.table"));
    };
    let schema = bracketed(schema)?;
    let name = bracketed(name)?;
    require_ident(schema)?;
    require_ident(name)?;
    if projection.is_empty() {
        return Err(Fail::new("not_product_sql", "load projection is empty"));
    }
    for part in projection.split(", ") {
        require_ident(bracketed(part)?)?;
    }
    Ok(())
}

fn bracketed(value: &str) -> Result<&str, Fail> {
    let Some(inner) = value.strip_prefix('[') else {
        return Err(Fail::new("not_product_sql", "identifier is not bracketed"));
    };
    let Some(inner) = inner.strip_suffix(']') else {
        return Err(Fail::new("not_product_sql", "identifier is not bracketed"));
    };
    if inner.contains('[') || inner.contains(']') {
        return Err(Fail::new("not_product_sql", "identifier is not bracketed"));
    }
    Ok(inner)
}

pub fn require_ident(value: &str) -> Result<(), Fail> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 128
        || !bytes[0].is_ascii_alphabetic() && bytes[0] != b'_'
        || !bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return Err(Fail::new("identifier", "identifier is not an ASCII name"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_reads_pass_and_writes_do_not() {
        assert!(admit_product_sql(SERVER_SQL).is_ok());
        assert!(admit_product_sql(CATALOG_SQL).is_ok());
        assert!(admit_product_sql(PERMISSION_SQL).is_ok());
        let load = load_sql("dbo", "orders", &["id".to_string(), "name_ci".to_string()], 10).unwrap();
        assert!(admit_product_sql(&load).is_ok());
        for sql in [
            "INSERT INTO dbo.t VALUES (1)",
            "UPDATE dbo.t SET a = 1",
            "DELETE FROM dbo.t",
            "CREATE TABLE dbo.t (a int)",
            "EXEC sp_cdc_enable_db",
            "SELECT * INTO dbo.copy FROM dbo.t",
            "WAITFOR DELAY '00:00:45'",
        ] {
            let err = admit_product_sql(sql).unwrap_err();
            assert_eq!(err.code, "write_sql", "{sql}");
        }
        assert_eq!(
            admit_product_sql("SELECT name FROM sys.tables").unwrap_err().code,
            "not_product_sql"
        );
    }

    #[test]
    fn negative_write_script_is_not_part_of_the_reader() {
        let product = include_str!("sql.rs").split("#[cfg(test)]").next().unwrap();
        assert!(!product.contains("02-negative-writes.sql"));
        for sql in [SERVER_SQL, CATALOG_SQL, PERMISSION_SQL] {
            assert!(!sql.to_ascii_lowercase().contains("waitfor"));
        }
        let load = load_sql("dbo", "orders", &["id".to_string()], 1).unwrap();
        assert!(!load.to_ascii_lowercase().contains("waitfor"));
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let script = root.join("reader").join("02-negative-writes.sql");
        assert!(!script.exists());
        for entry in std::fs::read_dir(root.join("src")).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            let body = text.split("#[cfg(test)]").next().unwrap();
            assert!(!body.contains("02-negative-writes.sql"), "{}", path.display());
        }
    }
}
