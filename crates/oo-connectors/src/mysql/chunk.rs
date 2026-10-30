use super::{MysqlError, mysql_ident};
use mysql_async::{Conn, Params, Value, prelude::Queryable};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, time::Duration};

type Result<T> = std::result::Result<T, MysqlError>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyType {
    Int,
    Text,
    Bytes,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyColumn {
    pub name: String,
    pub kind: KeyType,
    pub collation: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyValue {
    Int(i128),
    Text(Vec<u8>),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkBound {
    pub lo: Option<Vec<KeyValue>>,
    pub hi: Option<Vec<KeyValue>>,
}

fn validate(columns: &[KeyColumn], values: Option<&[KeyValue]>) -> Result<()> {
    if columns.is_empty()
        || columns.iter().enumerate().any(|(i, c)| {
            !mysql_ident(&c.name) || columns[..i].iter().any(|other| other.name == c.name)
        })
    {
        return Err(MysqlError::new("config", "chunk_key"));
    }
    if let Some(values) = values
        && (values.len() != columns.len()
            || columns.iter().zip(values).any(|(c, v)| {
                !matches!(
                    (&c.kind, v),
                    (KeyType::Int, KeyValue::Int(_))
                        | (KeyType::Text, KeyValue::Text(_))
                        | (KeyType::Bytes, KeyValue::Bytes(_))
                )
            }))
    {
        return Err(MysqlError::new("config", "chunk_bound"));
    }
    Ok(())
}

pub fn range_where(columns: &[KeyColumn], bound: &ChunkBound) -> Result<(String, Vec<KeyValue>)> {
    validate(columns, bound.lo.as_deref())?;
    validate(columns, bound.hi.as_deref())?;
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    for (values, lower) in [(&bound.lo, true), (&bound.hi, false)] {
        if let Some(values) = values {
            let mut alternatives = Vec::new();
            for i in 0..columns.len() {
                let mut terms = Vec::new();
                for j in 0..=i {
                    let op = if j < i {
                        "="
                    } else if lower {
                        ">"
                    } else if i + 1 == columns.len() {
                        "<="
                    } else {
                        "<"
                    };
                    terms.push(format!("`{}` {op} ?", columns[j].name));
                    params.push(values[j].clone());
                }
                alternatives.push(format!("({})", terms.join(" AND ")));
            }
            clauses.push(format!("({})", alternatives.join(" OR ")));
        }
    }
    Ok((
        if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        },
        params,
    ))
}

fn driver_params(values: Vec<KeyValue>) -> Result<Params> {
    values
        .into_iter()
        .map(|v| {
            Ok(match v {
                KeyValue::Int(n) if n < 0 => Value::Int(
                    i64::try_from(n).map_err(|_| MysqlError::new("config", "key_integer"))?,
                ),
                KeyValue::Int(n) => Value::UInt(
                    u64::try_from(n).map_err(|_| MysqlError::new("config", "key_integer"))?,
                ),
                KeyValue::Text(v) | KeyValue::Bytes(v) => Value::Bytes(v),
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(Params::Positional)
}

pub async fn key_columns(conn: &mut Conn, schema: &str, table: &str) -> Result<Vec<KeyColumn>> {
    if !mysql_ident(schema) || !mysql_ident(table) {
        return Err(MysqlError::new("config", "chunk_key"));
    }
    let sql = "SELECT s.INDEX_NAME FROM information_schema.STATISTICS s LEFT JOIN information_schema.COLUMNS c ON c.TABLE_SCHEMA=s.TABLE_SCHEMA AND c.TABLE_NAME=s.TABLE_NAME AND c.COLUMN_NAME=s.COLUMN_NAME WHERE s.TABLE_SCHEMA=? AND s.TABLE_NAME=? AND s.NON_UNIQUE=0 GROUP BY s.INDEX_NAME HAVING COUNT(*)=SUM(c.IS_NULLABLE='NO' AND s.SUB_PART IS NULL AND c.DATA_TYPE IN ('tinyint','smallint','mediumint','int','bigint','char','varchar','binary','varbinary')) ORDER BY (s.INDEX_NAME='PRIMARY') DESC,s.INDEX_NAME LIMIT 1";
    let index: Option<String> = conn
        .exec_first(sql, (schema, table))
        .await
        .map_err(|_| MysqlError::new("catalog", "chunk_index"))?;
    let Some(index) = index else {
        return Ok(Vec::new());
    };
    let rows:Vec<(String,String,Option<String>)>=conn.exec("SELECT c.COLUMN_NAME,c.DATA_TYPE,c.COLLATION_NAME FROM information_schema.STATISTICS s JOIN information_schema.COLUMNS c ON c.TABLE_SCHEMA=s.TABLE_SCHEMA AND c.TABLE_NAME=s.TABLE_NAME AND c.COLUMN_NAME=s.COLUMN_NAME WHERE s.TABLE_SCHEMA=? AND s.TABLE_NAME=? AND s.INDEX_NAME=? ORDER BY s.SEQ_IN_INDEX",(schema,table,index)).await.map_err(|_|MysqlError::new("catalog","chunk_index"))?;
    rows.into_iter()
        .map(|(name, data_type, collation)| {
            if !mysql_ident(&name) {
                return Err(MysqlError::new("catalog", "chunk_key"));
            }
            let kind = match data_type.as_str() {
                "char" | "varchar" => KeyType::Text,
                "binary" | "varbinary" => KeyType::Bytes,
                "tinyint" | "smallint" | "mediumint" | "int" | "bigint" => KeyType::Int,
                _ => return Err(MysqlError::new("catalog", "chunk_key")),
            };
            Ok(KeyColumn {
                name,
                kind,
                collation,
            })
        })
        .collect()
}

pub fn next_boundary_sql(
    schema: &str,
    table: &str,
    columns: &[KeyColumn],
    lo: Option<Vec<KeyValue>>,
    rows: u64,
) -> Result<(String, Vec<KeyValue>)> {
    if !mysql_ident(schema) || !mysql_ident(table) || rows == 0 || rows > 100_000 {
        return Err(MysqlError::new("config", "chunk_budget"));
    }
    let (predicate, params) = range_where(columns, &ChunkBound { lo, hi: None })?;
    let list = columns
        .iter()
        .map(|c| format!("`{}`", c.name))
        .collect::<Vec<_>>()
        .join(",");
    let sql = if columns.len() == 1 {
        format!(
            "SELECT MAX({list}) FROM (SELECT {list} FROM `{schema}`.`{table}`{predicate} ORDER BY {list} LIMIT {rows}) x"
        )
    } else {
        let desc = columns
            .iter()
            .map(|c| format!("`{}` DESC", c.name))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "SELECT {list} FROM (SELECT {list} FROM `{schema}`.`{table}`{predicate} ORDER BY {list} LIMIT {rows}) x ORDER BY {desc} LIMIT 1"
        )
    };
    Ok((sql, params))
}

pub async fn next_boundary(
    conn: &mut Conn,
    schema: &str,
    table: &str,
    columns: &[KeyColumn],
    lo: Option<Vec<KeyValue>>,
    rows: u64,
) -> Result<Option<Vec<KeyValue>>> {
    let (sql, params) = next_boundary_sql(schema, table, columns, lo, rows)?;
    let row: Option<mysql_async::Row> = conn
        .exec_first(sql, driver_params(params)?)
        .await
        .map_err(|_| MysqlError::new("protocol", "chunk_boundary"))?;
    let Some(row) = row else { return Ok(None) };
    if matches!(row.as_ref(0), Some(Value::NULL)) {
        return Ok(None);
    }
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| match (&c.kind, row.as_ref(i)) {
            (KeyType::Int, Some(Value::Int(n))) => Ok(KeyValue::Int(i128::from(*n))),
            (KeyType::Int, Some(Value::UInt(n))) => Ok(KeyValue::Int(i128::from(*n))),
            (KeyType::Text, Some(Value::Bytes(v))) => Ok(KeyValue::Text(v.clone())),
            (KeyType::Bytes, Some(Value::Bytes(v))) => Ok(KeyValue::Bytes(v.clone())),
            _ => Err(MysqlError::new("catalog", "chunk_key_value")),
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkEntry {
    pub table: String,
    pub chunk_id: u64,
    pub lo: Option<Vec<KeyValue>>,
    pub hi: Option<Vec<KeyValue>>,
    pub rows: u64,
    pub bytes: u64,
    pub arrow_sha256: String,
}

pub fn file_sha256(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).map_err(|_| MysqlError::new("resume", "arrow_open"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| MysqlError::new("resume", "arrow_read"))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn verify_arrow(path: &Path, expected: &str) -> Result<()> {
    if file_sha256(path)? != expected {
        return Err(MysqlError::new("resume", "arrow_sha256"));
    }
    Ok(())
}

pub struct AdaptiveRows {
    rows: u64,
    floor: u64,
    rate: Option<f64>,
    max_rows: u64,
    max_bytes: u64,
}
impl AdaptiveRows {
    pub fn new(initial: u64, max_rows: u64, max_bytes: u64) -> Result<Self> {
        if initial == 0
            || max_rows == 0
            || max_rows > 100_000
            || max_bytes == 0
            || max_bytes > 32 * 1024 * 1024
        {
            return Err(MysqlError::new("config", "chunk_budget"));
        }
        Ok(Self {
            rows: initial.min(max_rows),
            floor: initial.min(max_rows),
            rate: None,
            max_rows,
            max_bytes,
        })
    }
    pub fn rows(&self) -> u64 {
        self.rows
    }
    pub fn shrink(&mut self) -> bool {
        if self.rows <= 1 {
            return false;
        }
        self.rows = (self.rows / 2).max(1);
        self.floor = self.floor.min(self.rows);
        self.rate = None;
        true
    }
    pub fn observe(&mut self, rows: u64, bytes: u64, elapsed: Duration) {
        if rows == 0 || elapsed.is_zero() {
            return;
        }
        let rate = rows as f64 / elapsed.as_secs_f64();
        let rate = self.rate.map_or(rate, |old| old * 0.75 + rate * 0.25);
        self.rate = Some(rate);
        let byte_cap = self.max_bytes / bytes.div_ceil(rows).max(1);
        self.rows = ((rate * 3.0) as u64)
            .max(self.floor)
            .min(self.max_rows)
            .min(byte_cap.max(1));
    }
}

pub struct ChunkRead<'a> {
    pub schema: &'a str,
    pub table: &'a str,
    pub keys: &'a [KeyColumn],
    pub columns: &'a [super::map::MappedColumn],
    pub binding: &'a crate::types::RunBinding,
    pub bound: &'a ChunkBound,
    pub max_rows: u64,
    pub max_bytes: u64,
    pub deadline: Duration,
}

pub async fn read_chunk(mut conn: Conn, read: ChunkRead<'_>) -> Result<(Conn, super::Batch)> {
    let result = tokio::time::timeout(read.deadline, read_snapshot(&mut conn, &read, None))
        .await
        .map_err(|_| MysqlError::new("budget", "chunk_deadline"))
        .and_then(|r| r);
    match result {
        Ok(batch) => Ok((conn, batch)),
        Err(error) => {
            let _ = tokio::time::timeout(Duration::from_secs(5), conn.disconnect()).await;
            Err(error)
        }
    }
}

pub async fn read_c5_range(
    mut conn: Conn,
    read: ChunkRead<'_>,
    server_timeout_ms: u32,
) -> Result<(Conn, super::Batch, Vec<String>)> {
    if !(1..=30000).contains(&server_timeout_ms) {
        return Err(MysqlError::new("config", "chunk_timeout"));
    }
    let mut grants = Vec::new();
    let result = tokio::time::timeout(
        read.deadline,
        read_snapshot(&mut conn, &read, Some((server_timeout_ms, &mut grants))),
    )
    .await
    .map_err(|_| MysqlError::new("budget", "chunk_deadline"))
    .and_then(|r| r);
    match result {
        Ok(batch) => Ok((conn, batch, grants)),
        Err(error) => {
            let _ = tokio::time::timeout(Duration::from_secs(5), conn.disconnect()).await;
            Err(error)
        }
    }
}

async fn read_snapshot(
    conn: &mut Conn,
    read: &ChunkRead<'_>,
    mut c5: Option<(u32, &mut Vec<String>)>,
) -> Result<super::Batch> {
    if !mysql_ident(read.schema)
        || !mysql_ident(read.table)
        || read.columns.is_empty()
        || read.columns.iter().any(|c| !mysql_ident(&c.plan.name))
        || read.binding.schema != read.schema
        || read.binding.table != read.table
        || read.max_rows == 0
        || read.max_rows > 100_000
        || read.max_bytes == 0
        || read.max_bytes > 16 * 1024 * 1024
        || read.bound.hi.is_none() && c5.is_none()
    {
        return Err(MysqlError::new("config", "chunk_read"));
    }
    if read.keys.len() != read.columns.iter().filter(|c| c.key).count() {
        return Err(MysqlError::new("catalog", "chunk_key"));
    }
    for key in read.keys {
        if !read
            .columns
            .iter()
            .any(|c| c.plan.name == key.name && c.key && c.not_null)
        {
            return Err(MysqlError::new("catalog", "chunk_key"));
        }
    }
    let (mut predicate, params) = range_where(read.keys, read.bound)?;
    if c5.is_some() && predicate.is_empty() {
        predicate = " WHERE 1".into();
    }
    for sql in [
        "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
    ] {
        conn.query_drop(sql)
            .await
            .map_err(|_| MysqlError::new("protocol", "chunk_snapshot"))?;
    }
    if let Some((_, grants)) = c5.as_mut() {
        **grants = conn
            .query("SHOW GRANTS FOR CURRENT_USER()")
            .await
            .map_err(|_| MysqlError::new("privilege", "grants"))?;
        super::full_read_grants(grants)?;
    }
    let engine: Option<String> = conn.exec_first("SELECT ENGINE FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND TABLE_TYPE = 'BASE TABLE'", (read.schema, read.table)).await.map_err(|_| MysqlError::new("catalog", "engine"))?;
    if engine.as_deref() != Some("InnoDB") {
        return Err(MysqlError::new("catalog", "snapshot_engine"));
    }
    let expressions = read
        .columns
        .iter()
        .map(super::map::capture_expression)
        .collect::<Vec<_>>();
    let sizes = read
        .columns
        .iter()
        .zip(&expressions)
        .map(|(c, expression)| {
            if matches!(
                c.store,
                super::map::StoreKind::Text { .. }
                    | super::map::StoreKind::Json
                    | super::map::StoreKind::Bytes
            ) {
                let alias = format!(" AS `{}`", c.plan.name);
                let value = expression.strip_suffix(&alias).unwrap_or(expression);
                format!("COALESCE(OCTET_LENGTH({value}),8)")
            } else {
                "8".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("+");
    let list = expressions.join(",");
    let order = read
        .keys
        .iter()
        .map(|c| format!("`{}`", c.name))
        .collect::<Vec<_>>()
        .join(",");
    let size_sql = format!(
        "SELECT MAX(__oo_row_bytes) FROM (SELECT ({sizes}) AS __oo_row_bytes FROM `{}`.`{}`{predicate} ORDER BY {order} LIMIT {}) x",
        read.schema,
        read.table,
        read.max_rows + 1
    );
    let largest: Option<Option<u64>> = conn
        .exec_first(size_sql, driver_params(params.clone())?)
        .await
        .map_err(|_| MysqlError::new("protocol", "row_bytes"))?;
    if let Some(value_len) = largest.flatten().filter(|bytes| *bytes > read.max_bytes) {
        let mut error = MysqlError::new("budget", "max_bytes");
        error.value_len = Some(value_len);
        return Err(error);
    }
    let hint = c5
        .as_ref()
        .map(|(ms, _)| format!("/*+ MAX_EXECUTION_TIME({ms}) */ "))
        .unwrap_or_default();
    let sql = format!(
        "SELECT {hint}{list} FROM `{}`.`{}`{predicate} ORDER BY {order} LIMIT {}",
        read.schema,
        read.table,
        read.max_rows + 1
    );
    let mut result = conn
        .exec_iter(&sql, driver_params(params)?)
        .await
        .map_err(|_| MysqlError::new("protocol", "chunk_read"))?;
    let mut rows = Vec::new();
    let mut bytes = 0u64;
    while let Some(row) = result
        .next()
        .await
        .map_err(|_| MysqlError::new("protocol", "chunk_read"))?
    {
        if rows.len() as u64 == read.max_rows {
            return Err(MysqlError::new("budget", "max_rows"));
        }
        let row_bytes = (0..read.columns.len()).try_fold(0u64, |n, i| {
            Ok::<_, MysqlError>(n.saturating_add(match row.as_ref(i) {
                Some(Value::Bytes(v)) => v.len() as u64,
                Some(_) => 8,
                None => return Err(MysqlError::new("protocol", "row_width")),
            }))
        })?;
        bytes = bytes.saturating_add(row_bytes);
        if bytes > read.max_bytes {
            return Err(MysqlError::new("budget", "max_bytes"));
        }
        rows.push(row);
    }
    result
        .drop_result()
        .await
        .map_err(|_| MysqlError::new("protocol", "chunk_read"))?;
    let batch = super::reader::encode_chunk_batch(read.columns, read.binding, rows, bytes, &sql)?;
    if batch
        .typed
        .record_batch()
        .map_err(|_| MysqlError::new("batch", "arrow"))?
        .get_array_memory_size() as u64
        > read.max_bytes
    {
        return Err(MysqlError::new("budget", "arrow_bytes"));
    }
    conn.query_drop("COMMIT")
        .await
        .map_err(|_| MysqlError::new("protocol", "chunk_commit"))?;
    Ok(batch)
}

fn validate_manifest(
    directory: &Path,
    table: &str,
    entries: &[ChunkEntry],
    verify_files: bool,
) -> Result<()> {
    let mut previous = None;
    for (index, entry) in entries.iter().enumerate() {
        if entry.table != table
            || entry.chunk_id != index as u64
            || entry.lo != previous
            || entry.hi.is_none()
            || entry.rows == 0
            || entry.arrow_sha256.len() != 64
        {
            return Err(MysqlError::new("resume", "manifest_chain"));
        }
        if verify_files || index + 1 == entries.len() {
            verify_arrow(
                &directory.join(format!("{}.arrow", entry.chunk_id)),
                &entry.arrow_sha256,
            )?;
        }
        previous = entry.hi.clone();
    }
    Ok(())
}

pub fn load_manifest(directory: &Path, table: &str) -> Result<Vec<ChunkEntry>> {
    let path = directory.join("chunks.json");
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(MysqlError::new("resume", "manifest_open")),
    };
    if file
        .metadata()
        .map_err(|_| MysqlError::new("resume", "manifest_read"))?
        .len()
        > 32 * 1024 * 1024
    {
        return Err(MysqlError::new("resume", "manifest_size"));
    }
    let entries: Vec<ChunkEntry> =
        serde_json::from_reader(file).map_err(|_| MysqlError::new("resume", "manifest_json"))?;
    validate_manifest(directory, table, &entries, true)?;
    Ok(entries)
}

pub fn save_manifest(directory: &Path, table: &str, entries: &[ChunkEntry]) -> Result<()> {
    use std::io::Write;
    validate_manifest(directory, table, entries, false)?;
    let bytes =
        serde_json::to_vec(entries).map_err(|_| MysqlError::new("resume", "manifest_json"))?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(MysqlError::new("resume", "manifest_size"));
    }
    let temp = directory.join("chunks.json.pending");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| MysqlError::new("resume", "manifest_pending"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| MysqlError::new("resume", "manifest_write"))?;
    std::fs::rename(temp, directory.join("chunks.json"))
        .map_err(|_| MysqlError::new("resume", "manifest_rename"))?;
    std::fs::File::open(directory)
        .and_then(|f| f.sync_all())
        .map_err(|_| MysqlError::new("resume", "manifest_sync"))?;
    Ok(())
}

#[cfg(test)]
mod typed_key_order_tests {
    use super::*;

    #[test]
    fn textual_boundary_is_ordered_by_source_column_not_binary_identity() {
        let keys=vec![KeyColumn{name:"k".into(),kind:KeyType::Text,
            collation:Some("utf8mb4_unicode_ci".into())}];
        let lo=vec![KeyValue::Text(b"a".to_vec())];
        let (sql,params)=next_boundary_sql("owned","items",&keys,Some(lo.clone()),100).unwrap();
        assert_eq!(sql,"SELECT MAX(`k`) FROM (SELECT `k` FROM `owned`.`items` WHERE ((`k` > ?)) ORDER BY `k` LIMIT 100) x");
        assert_eq!(params,lo);
        assert!(!sql.contains("BINARY") && !sql.contains("HEX") && !sql.contains("COLLATE"));
    }

    #[test]
    fn composite_bounds_keep_bytes_uint64_and_repeated_lexicographic_parameters() {
        let keys=vec![
            KeyColumn{name:"tenant".into(),kind:KeyType::Text,collation:Some("utf8mb4_unicode_ci".into())},
            KeyColumn{name:"part".into(),kind:KeyType::Bytes,collation:None},
            KeyColumn{name:"seq".into(),kind:KeyType::Int,collation:None},
        ];
        let lo=vec![KeyValue::Text("\u{4e2d}".as_bytes().to_vec()),
            KeyValue::Bytes(vec![0,255,128]),KeyValue::Int(i128::from(u64::MAX))];
        let (sql,params)=next_boundary_sql("owned","mixed",&keys,Some(lo.clone()),17).unwrap();
        assert!(sql.contains("ORDER BY `tenant`,`part`,`seq` LIMIT 17"));
        assert!(sql.ends_with("ORDER BY `tenant` DESC,`part` DESC,`seq` DESC LIMIT 1"));
        assert_eq!(params,vec![lo[0].clone(),lo[0].clone(),lo[1].clone(),
            lo[0].clone(),lo[1].clone(),lo[2].clone()]);
        match driver_params(lo).unwrap() {
            Params::Positional(p)=>assert_eq!(p,vec![Value::Bytes("\u{4e2d}".as_bytes().to_vec()),
                Value::Bytes(vec![0,255,128]),Value::UInt(u64::MAX)]),
            other=>panic!("unexpected parameters: {other:?}"),
        }
    }

    #[test]
    fn collation_inverted_bounds_are_not_compared_in_rust() {
        let keys=vec![KeyColumn{name:"k".into(),kind:KeyType::Text,
            collation:Some("utf8mb4_unicode_ci".into())}];
        let lo=KeyValue::Text(b"a".to_vec());let hi=KeyValue::Text(b"B".to_vec());
        let (sql,params)=range_where(&keys,&ChunkBound{lo:Some(vec![lo.clone()]),hi:Some(vec![hi.clone()])}).unwrap();
        assert_eq!(sql," WHERE ((`k` > ?)) AND ((`k` <= ?))");
        assert_eq!(params,vec![lo,hi]);
    }

    #[test]
    fn missing_or_mistyped_keys_cannot_become_an_unbounded_scan() {
        assert_eq!(next_boundary_sql("owned","no_key",&[],None,100).unwrap_err().fact,"chunk_key");
        let keys=vec![KeyColumn{name:"k".into(),kind:KeyType::Bytes,collation:None}];
        assert_eq!(next_boundary_sql("owned","items",&keys,
            Some(vec![KeyValue::Text(vec![0,255])]),100).unwrap_err().fact,"chunk_bound");
    }
}
