use oracledb::{Connection, OracleNumber, OracleTimestamp};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

pub type Result<T> = std::result::Result<T, String>;
pub const PROTOCOL: &str = "oo.oracle.bounded-chunk/v1";

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn ident(name: &str) -> Result<String> {
    // Quoted/mixed-case identifiers are supported; never interpolate unchecked SQL.
    if name.is_empty()
        || name.len() > 128
        || name.contains('"')
        || name.chars().any(char::is_control)
    {
        return Err("invalid_identifier".into());
    }
    Ok(format!("\"{name}\""))
}

pub fn text(v: &Value, key: &str) -> Result<String> {
    v[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("missing_{key}"))
}

pub fn db_error(e: oracledb::Error) -> String {
    // Driver errors may contain connect descriptors or values. Keep only ORA codes.
    let message = e.to_string();
    if let Some(start) = message.find("ORA-") {
        return message[start..].chars().take(9).collect();
    }
    "oracle_driver_error".into()
}

pub fn strings(conn: &Connection, sql: &str) -> Result<Vec<String>> {
    let cursor = conn.query(sql, &[]).map_err(db_error)?;
    let mut out = Vec::new();
    for row in cursor {
        if out.len() == 65536 {
            return Err("catalog_budget".into());
        }
        out.push(row.map_err(db_error)?.get::<String>(0).map_err(db_error)?);
    }
    Ok(out)
}

fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub fn identity(conn: &Connection) -> Result<String> {
    strings(conn, "SELECT TO_CHAR(dbid)||'|'||TO_CHAR(resetlogs_change#)||'|'||(SELECT RAWTOHEX(guid) FROM v$pdbs WHERE con_id=TO_NUMBER(SYS_CONTEXT('USERENV','CON_ID')))||'|'||SYS_CONTEXT('USERENV','CON_NAME')||'|'||db_unique_name||'|'||SYS_CONTEXT('USERENV','SESSION_USER') FROM v$database")?
        .into_iter().next().ok_or("identity_absent".into())
}

pub fn scn(conn: &Connection) -> Result<u64> {
    strings(conn, "SELECT TO_CHAR(current_scn) FROM v$database")?
        .first()
        .ok_or("scn_absent")?
        .parse()
        .map_err(|_| "scn_invalid".into())
}

/// Catalog coverage is an admission check, not proof that LogMiner read a file.
pub fn log_catalog(conn: &Connection) -> Result<Value> {
    let mut ranges = Vec::new();
    for raw in strings(
        conn,
        "SELECT JSON_OBJECT('kind' VALUE 'archived','thread' VALUE thread#,'sequence' VALUE sequence#,'first_scn' VALUE TO_CHAR(first_change#),'next_scn' VALUE TO_CHAR(next_change#) RETURNING VARCHAR2(4000)) FROM v$archived_log WHERE deleted='NO' AND status='A' AND name IS NOT NULL AND standby_dest='NO' UNION ALL SELECT JSON_OBJECT('kind' VALUE 'online','thread' VALUE thread#,'sequence' VALUE sequence#,'first_scn' VALUE TO_CHAR(first_change#),'next_scn' VALUE TO_CHAR(next_change#) RETURNING VARCHAR2(4000)) FROM v$log WHERE status IN ('CURRENT','ACTIVE','INACTIVE')",
    )? {
        ranges.push(serde_json::from_str::<Value>(&raw).map_err(|_| "log_catalog_json")?);
    }
    Ok(
        json!({"protocol":"oo.oracle.log-catalog/v1","current_scn":scn(conn)?.to_string(),
              "ranges":ranges,"catalog_only":true,"reader_mutations":0,
              "handoff_requires":"actual restored vendor checkpoint and complete LogMiner END"}),
    )
}

pub fn inventory(conn: &Connection, owner: &str) -> Result<Vec<Value>> {
    ident(owner)?;
    let owner_lit = literal(owner);
    let tables = strings(
        conn,
        &format!("SELECT table_name FROM all_tables WHERE owner={owner_lit} ORDER BY table_name"),
    )?;
    let mut result = Vec::new();
    for table in tables {
        let table_lit = literal(&table);
        let columns = strings(conn, &format!("SELECT JSON_OBJECT('name' VALUE column_name,'type' VALUE data_type,'precision' VALUE data_precision,'scale' VALUE data_scale,'bytes' VALUE data_length,'nullable' VALUE nullable,'collation' VALUE collation,'position' VALUE internal_column_id,'hidden' VALUE hidden_column,'virtual' VALUE virtual_column,'user_generated' VALUE user_generated NULL ON NULL RETURNING VARCHAR2(4000)) FROM all_tab_cols WHERE owner={owner_lit} AND table_name={table_lit} ORDER BY internal_column_id"))?
            .into_iter().map(|s| serde_json::from_str(&s).map_err(|_| "catalog_json".into())).collect::<Result<Vec<Value>>>()?;
        let keys = strings(
            conn,
            &format!(
                "SELECT cc.column_name FROM all_constraints c JOIN all_cons_columns cc ON c.owner=cc.owner AND c.constraint_name=cc.constraint_name AND c.table_name=cc.table_name WHERE c.owner={owner_lit} AND c.table_name={table_lit} AND c.constraint_type='P' AND c.status='ENABLED' AND c.validated='VALIDATED' ORDER BY cc.position"
            ),
        )?;
        let object = strings(
            conn,
            &format!(
                "SELECT TO_CHAR(object_id)||'|'||TO_CHAR(last_ddl_time,'YYYY-MM-DD HH24:MI:SS') FROM all_objects WHERE owner={owner_lit} AND object_name={table_lit} AND object_type='TABLE'"
            ),
        )?;
        let mut issues = Vec::new();
        if columns.is_empty() || columns.len() > 256 {
            issues.push("column_budget".to_string());
        }
        if keys.is_empty() {
            issues.push("no_validated_primary_key: explicit exclusion or separately approved full-replacement required; ROWID is not a durable key".into());
        }
        for column in &columns {
            let name = text(column, "name")?;
            let kind = text(column, "type")?;
            if column["user_generated"] == "NO" || column["virtual"] == "YES" {
                issues.push(format!(
                    "derived_or_system_column:{name}: explicit policy required; never silently omit"
                ));
            }
            if !supported(&kind) {
                issues.push(format!("unsupported_type:{name}:{kind}: no truncation; LOB/LONG require a separate streaming contract"));
            }
            if keys.contains(&name) {
                if !matches!(kind.as_str(), "NUMBER" | "VARCHAR2" | "NVARCHAR2" | "RAW")
                    || column["nullable"] != "N"
                {
                    issues.push(format!("unsupported_key:{name}"));
                }
                if kind.contains("CHAR")
                    && !matches!(
                        column["collation"].as_str(),
                        Some("BINARY" | "USING_NLS_COMP")
                    )
                {
                    issues.push(format!("non_binary_key_collation:{name}"));
                }
            }
        }
        result.push(json!({"owner":owner,"table":table,"object":object,"columns":columns,"keys":keys,"issues":issues}));
    }
    Ok(result)
}

pub fn supported(kind: &str) -> bool {
    matches!(
        kind,
        "NUMBER" | "DATE" | "RAW" | "VARCHAR2" | "NVARCHAR2" | "CHAR" | "NCHAR"
    ) || (0..=9).any(|n| {
        kind == format!("TIMESTAMP({n})") || kind == format!("TIMESTAMP({n}) WITH TIME ZONE")
    })
}

pub fn decimal(s: &str) -> Result<String> {
    let s = s.trim();
    if s.len() > 256 {
        return Err("number_length".into());
    }
    let (negative, bare) = if let Some(v) = s.strip_prefix('-') {
        (true, v)
    } else {
        (false, s)
    };
    let (mantissa, exponent) = match bare.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i32>().map_err(|_| "number_exponent")?),
        None => (bare, 0),
    };
    if !(-130..=125).contains(&exponent) {
        return Err("number_exponent".into());
    }
    let mut split = mantissa.split('.');
    let whole = split.next().unwrap_or("");
    let frac = split.next().unwrap_or("");
    if split.next().is_some()
        || whole.len() + frac.len() == 0
        || !whole
            .bytes()
            .chain(frac.bytes())
            .all(|x| x.is_ascii_digit())
    {
        return Err("number_syntax".into());
    }
    let digits = format!("{whole}{frac}");
    let point = whole.len() as i32 + exponent;
    let plain = if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!("{digits}{}", "0".repeat(point as usize - digits.len()))
    } else {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    };
    let (whole, frac) = plain.split_once('.').unwrap_or((&plain, ""));
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let frac = frac.trim_end_matches('0');
    let value = if frac.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{frac}")
    };
    Ok(if negative && value != "0" {
        format!("-{value}")
    } else {
        value
    })
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn key_literal(column: &Value, value: &Value) -> Result<String> {
    let value = value.as_str().ok_or("null_or_untyped_key")?;
    if value.len() > 8192 {
        return Err("key_byte_budget".into());
    }
    match text(column, "type")?.as_str() {
        "NUMBER" => Ok(decimal(value)?),
        "RAW" => {
            if value.len() % 2 != 0 || !value.bytes().all(|x| x.is_ascii_hexdigit()) {
                return Err("raw_key".into());
            }
            Ok(format!("HEXTORAW('{value}')"))
        }
        "VARCHAR2" => Ok(format!(
            "UTL_I18N.RAW_TO_CHAR(HEXTORAW('{}'),'AL32UTF8')",
            hex(value.as_bytes())
        )),
        "NVARCHAR2" => Ok(format!(
            "TO_NCHAR(UTL_I18N.RAW_TO_CHAR(HEXTORAW('{}'),'AL32UTF8'))",
            hex(value.as_bytes())
        )),
        _ => Err("unsupported_key".into()),
    }
}

pub fn key_indices(table: &Value) -> Result<Vec<usize>> {
    let columns = table["columns"].as_array().ok_or("columns")?;
    table["keys"]
        .as_array()
        .ok_or("keys")?
        .iter()
        .map(|k| {
            columns
                .iter()
                .position(|c| c["name"] == *k)
                .ok_or("missing_key_column".into())
        })
        .collect()
}

pub fn lower_clause(table: &Value, lower: &Value) -> Result<String> {
    let indices = key_indices(table)?;
    if indices.is_empty() {
        return Err("no_key".into());
    }
    if lower.is_null() {
        return Ok(String::new());
    }
    let values = lower.as_array().ok_or("key_shape")?;
    if values.len() != indices.len() {
        return Err("key_arity".into());
    }
    let mut alternatives = Vec::new();
    for end in 0..indices.len() {
        let mut terms = Vec::new();
        for j in 0..=end {
            let column = &table["columns"][indices[j]];
            terms.push(format!(
                "{} {} {}",
                ident(&text(column, "name")?)?,
                if j == end { ">" } else { "=" },
                key_literal(column, &values[j])?
            ));
        }
        alternatives.push(format!("({})", terms.join(" AND ")));
    }
    Ok(format!(" WHERE ({})", alternatives.join(" OR ")))
}

fn projection(column: &Value) -> Result<String> {
    let name = ident(&text(column, "name")?)?;
    let kind = text(column, "type")?;
    Ok(match kind.as_str() {
        "NUMBER" => format!("TO_CHAR({name},'TM9','NLS_NUMERIC_CHARACTERS=''.,''')"),
        "RAW" => format!("RAWTOHEX({name})"),
        "DATE" => {
            format!("TO_CHAR({name},'SYYYY-MM-DD\"T\"HH24:MI:SS','NLS_DATE_LANGUAGE=American')")
        }
        "VARCHAR2" | "CHAR" | "NCHAR" | "NVARCHAR2" => name,
        _ if supported(&kind) => format!(
            "TO_CHAR({name},'SYYYY-MM-DD\"T\"HH24:MI:SS.FF9{}','NLS_DATE_LANGUAGE=American')",
            if kind.ends_with("WITH TIME ZONE") {
                " TZH:TZM"
            } else {
                ""
            }
        ),
        _ => return Err("unsupported_type".into()),
    })
}

pub fn select_sql(
    table: &Value,
    scn: u64,
    lower: &Value,
    rows: usize,
    projected: bool,
) -> Result<String> {
    if scn == 0
        || !(1..=4096).contains(&rows)
        || !table["issues"].as_array().ok_or("issues")?.is_empty()
    {
        return Err("snapshot_admission".into());
    }
    let columns = table["columns"].as_array().ok_or("columns")?;
    let expressions = columns
        .iter()
        .map(|c| {
            if projected {
                projection(c)
            } else {
                ident(&text(c, "name")?)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let keys = table["keys"]
        .as_array()
        .ok_or("keys")?
        .iter()
        .map(|k| ident(k.as_str().ok_or("key_name")?))
        .collect::<Result<Vec<_>>>()?;
    Ok(format!(
        "SELECT {} FROM {}.{} AS OF SCN {}{} ORDER BY {} FETCH FIRST {} ROWS ONLY",
        expressions.join(","),
        ident(&text(table, "owner")?)?,
        ident(&text(table, "table")?)?,
        scn,
        lower_clause(table, lower)?,
        keys.join(","),
        rows
    ))
}

pub fn read_chunk(
    conn: &Connection,
    table: &Value,
    scn: u64,
    lower: &Value,
    rows: usize,
    bytes: usize,
    timeout: Duration,
) -> Result<Value> {
    if !(1024..=16 * 1024 * 1024).contains(&bytes) {
        return Err("byte_budget".into());
    }
    let sql = select_sql(table, scn, lower, rows, true)?;
    let columns = table["columns"].as_array().ok_or("columns")?;
    let start = Instant::now();
    // One-row driver fetch also bounds the transient buffer for a very wide row.
    let statement = conn
        .statement(&sql)
        .map_err(db_error)?
        .fetch_array_size(1)
        .prefetch_rows(0)
        .build()
        .map_err(db_error)?;
    let cursor = statement.query(&[]).map_err(db_error)?;
    let mut output = Vec::new();
    let mut used = 0;
    for row in cursor {
        if start.elapsed() > timeout {
            return Err("chunk_deadline".into());
        }
        let row = row.map_err(db_error)?;
        let mut cells = Vec::new();
        for (i, column) in columns.iter().enumerate() {
            let cell: Option<String> = row.get(i).map_err(db_error)?;
            let kind = text(column, "type")?;
            cells.push(match cell {
                None => Value::Null,
                Some(v) => Value::String(match kind.as_str() {
                    "NUMBER" => decimal(&v)?,
                    "RAW" => v.to_ascii_lowercase(),
                    "DATE" => positive_era(&v)?,
                    _ if kind.starts_with("TIMESTAMP(") => positive_era(&v)?,
                    _ => v,
                }),
            });
        }
        used += serde_json::to_vec(&cells).map_err(|_| "row_encode")?.len() + 1;
        if used > bytes {
            return Err("chunk_byte_budget: reduce chunk_rows; never truncate".into());
        }
        output.push(Value::Array(cells));
    }
    let keys = key_indices(table)?;
    let next = output
        .last()
        .map(|r| Value::Array(keys.iter().map(|i| r[*i].clone()).collect()))
        .unwrap_or(lower.clone());
    if !output.is_empty() && next == *lower {
        return Err("key_not_advancing".into());
    }
    Ok(
        json!({"protocol":PROTOCOL,"scn":scn.to_string(),"table":table,"lower_exclusive":lower,"upper_inclusive":next,"rows":output,"row_count":output.len(),"logical_bytes":used,"eof":output.len()<rows,"max_fetch_rows":1,"reader_mutations":0}),
    )
}

fn positive_era(v: &str) -> Result<String> {
    if v.starts_with('-') {
        return Err("unsupported_BC_era".into());
    }
    Ok(v.trim_start_matches([' ', '+']).to_string())
}

fn stamp(t: &OracleTimestamp, fraction: bool) -> Result<String> {
    if t.year() < 1 {
        return Err("unsupported_BC_era".into());
    }
    let value = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    );
    Ok(if fraction {
        format!("{value}.{:09}", t.nanoseconds())
    } else {
        value
    })
}

/// Independent wire-typed check: no production projection/TO_CHAR row path.
pub fn verify_chunk(conn: &Connection, chunk: &Value, row_limit: usize) -> Result<usize> {
    let table = &chunk["table"];
    let expected = chunk["rows"].as_array().ok_or("rows")?;
    let scn = text(chunk, "scn")?.parse().map_err(|_| "scn")?;
    let sql = select_sql(table, scn, &chunk["lower_exclusive"], row_limit, false)?;
    let statement = conn
        .statement(&sql)
        .map_err(db_error)?
        .fetch_array_size(1)
        .prefetch_rows(0)
        .build()
        .map_err(db_error)?;
    let cursor = statement.query(&[]).map_err(db_error)?;
    let columns = table["columns"].as_array().ok_or("columns")?;
    let mut count = 0;
    for row in cursor {
        let row = row.map_err(db_error)?;
        let want = expected.get(count).ok_or("independent_extra_row")?;
        for (i, column) in columns.iter().enumerate() {
            let kind = text(column, "type")?;
            let actual = match kind.as_str() {
                "NUMBER" => row
                    .get::<Option<OracleNumber>>(i)
                    .map_err(db_error)?
                    .map(|n| decimal(&n.to_string()))
                    .transpose()?,
                "RAW" => row
                    .get::<Option<Vec<u8>>>(i)
                    .map_err(db_error)?
                    .map(|b| hex(&b)),
                "DATE" => row
                    .get::<Option<OracleTimestamp>>(i)
                    .map_err(db_error)?
                    .map(|t| stamp(&t, false))
                    .transpose()?,
                _ if kind.starts_with("TIMESTAMP(") => row
                    .get::<Option<OracleTimestamp>>(i)
                    .map_err(db_error)?
                    .map(|t| -> Result<String> {
                        let v = stamp(&t, true)?;
                        if kind.ends_with("WITH TIME ZONE") {
                            let h = t.tz_hour_offset();
                            let m = t.tz_minute_offset();
                            let v = super::timestamp::shift_timestamp(&v, h as i64, m as i64)?;
                            Ok(format!(
                                "{v} {}{:02}:{:02}",
                                if h < 0 || m < 0 { "-" } else { "+" },
                                h.abs(),
                                m.abs()
                            ))
                        } else {
                            Ok(v)
                        }
                    })
                    .transpose()?,
                _ => row.get::<Option<String>>(i).map_err(db_error)?,
            };
            if actual.as_deref() != want[i].as_str() || (actual.is_none() && !want[i].is_null()) {
                return Err(format!("typed_cell_mismatch:row={count}:column={i}"));
            }
        }
        count += 1;
    }
    if count != expected.len() {
        return Err("independent_missing_rows".into());
    }
    Ok(count * columns.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table() -> Value {
        json!({"owner":"S","table":"T","issues":[],"columns":[{"name":"A","type":"NUMBER"},{"name":"B","type":"VARCHAR2"}],"keys":["A","B"]})
    }
    #[test]
    fn decimal_is_exact_and_closed() {
        assert_eq!(
            decimal("9.9999999999999999999999999999999999999E+37").unwrap(),
            "99999999999999999999999999999999999999"
        );
        assert_eq!(decimal("-0.000000").unwrap(), "0");
        assert_eq!(
            decimal("1e-130").unwrap(),
            format!("0.{}1", "0".repeat(129))
        );
        for bad in ["NaN", "1;DROP TABLE X", "1.2.3", "1e9999", ""] {
            assert!(decimal(bad).is_err());
        }
    }
    #[test]
    fn composite_is_lexicographic_not_offset() {
        let s = select_sql(&table(), 17, &json!(["123", "x' OR 1=1"]), 73, true).unwrap();
        assert!(s.contains("(\"A\" > 123) OR (\"A\" = 123 AND \"B\" >"));
        assert!(s.contains("AS OF SCN 17"));
        assert!(s.ends_with("FETCH FIRST 73 ROWS ONLY"));
        assert!(!s.contains("OFFSET"));
        assert!(!s.contains("x' OR"));
    }
    #[test]
    fn rejects_unsafe_budget_key_type_and_arity() {
        assert!(select_sql(&table(), 0, &Value::Null, 10, true).is_err());
        assert!(select_sql(&table(), 1, &Value::Null, 4097, true).is_err());
        assert!(lower_clause(&table(), &json!(["1"])).is_err());
        assert!(lower_clause(&table(), &json!([null, "x"])).is_err());
        for t in [
            "CLOB",
            "NCLOB",
            "BLOB",
            "BFILE",
            "LONG",
            "XMLTYPE",
            "BINARY_DOUBLE",
            "TIMESTAMP(6) WITH LOCAL TIME ZONE",
        ] {
            assert!(!supported(t));
        }
        assert!(ident("a\"b").is_err());
        assert_eq!(ident("Mixed Table").unwrap(), "\"Mixed Table\"");
    }
}
