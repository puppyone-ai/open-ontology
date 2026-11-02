use mysql_async::{Conn, Params, TxOpts, Value, prelude::Queryable};
use oo_connectors::{
    mysql::{
        map::StoreKind,
        values::{RawCell, decode_cell},
    },
    types::DecodedCell,
};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn ident(s: &str) -> Result<String> {
    if s.is_empty() || s.contains('\0') {
        return Err("invalid identifier".into());
    }
    Ok(format!("`{}`", s.replace('`', "``")))
}

pub fn digest_sql(
    schema: &str,
    table: &str,
    keys: &[&str],
    columns: &[&str],
    predicate: &str,
    timeout_ms: u32,
) -> Result<String> {
    if keys.is_empty() || columns.is_empty() || keys.iter().any(|k| !columns.contains(k)) {
        return Err("digest must include every key".into());
    }
    let mut fields = Vec::new();
    for col in columns {
        let c = ident(col)?;
        fields.push(format!("COALESCE(HEX(CAST({c} AS BINARY)), '')"));
        fields.push(format!("ISNULL({c})"));
    }
    let md5 = format!("MD5(CONCAT_WS('#', {}))", fields.join(", "));
    Ok(format!(
        "SELECT /*+ MAX_EXECUTION_TIME({timeout_ms}) */ COUNT(*), BIT_XOR(CAST(CONV(SUBSTRING({md5},1,16),16,10) AS UNSIGNED)), BIT_XOR(CAST(CONV(SUBSTRING({md5},17,16),16,10) AS UNSIGNED)) FROM {}.{} WHERE ({predicate})",
        ident(schema)?,
        ident(table)?
    ))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub chunk_id: String,
    pub bound: Json,
    pub rows: u64,
    pub digest: [u64; 2],
    pub scanned_at: u64,
}
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    pub unchanged: Vec<String>,
    pub changed: Vec<String>,
    pub new: Vec<String>,
    pub removed: Vec<String>,
}
pub fn compare(previous: &[Chunk], current: &[Chunk]) -> Result<Changes> {
    fn index(chunks: &[Chunk]) -> Result<BTreeMap<&str, &Chunk>> {
        let mut map = BTreeMap::new();
        for c in chunks {
            if map.insert(c.chunk_id.as_str(), c).is_some() {
                return Err("duplicate chunk".into());
            }
        }
        Ok(map)
    }
    let old = index(previous)?;
    let now = index(current)?;
    let mut out = Changes::default();
    for (id, c) in &now {
        match old.get(id) {
            None => out.new.push((*id).into()),
            Some(p) if p.bound == c.bound && p.rows == c.rows && p.digest == c.digest => {
                out.unchanged.push((*id).into())
            }
            _ => out.changed.push((*id).into()),
        }
    }
    for id in old.keys() {
        if !now.contains_key(id) {
            out.removed.push((*id).into());
        }
    }
    Ok(out)
}
#[derive(Debug, PartialEq)]
pub enum Resize {
    Split,
    MergeCandidate,
    Keep,
}
pub fn resize(rows: u64, target: u64) -> Resize {
    if rows > target.saturating_mul(2) {
        Resize::Split
    } else if rows < target / 4 {
        Resize::MergeCandidate
    } else {
        Resize::Keep
    }
}

pub fn save(path: &Path, state: &Json) -> Result<()> {
    let temporary = path.with_extension("json.pending");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec(state)?)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
pub fn chunk_json(c: &Chunk) -> Json {
    json!({"chunk_id":c.chunk_id,"bound":c.bound,"rows":c.rows,"digest":[format!("{:016x}",c.digest[0]),format!("{:016x}",c.digest[1])],"scanned_at":c.scanned_at})
}
pub fn load_chunks(state: &Json) -> Result<Vec<Chunk>> {
    if state["version"] != 1 {
        return Err("unsupported state".into());
    }
    let mut out = Vec::new();
    for c in state["chunks"].as_array().ok_or("invalid chunks")? {
        out.push(Chunk {
            chunk_id: c["chunk_id"].as_str().ok_or("invalid id")?.into(),
            bound: c["bound"].clone(),
            rows: c["rows"].as_u64().ok_or("invalid rows")?,
            digest: [
                u64::from_str_radix(c["digest"][0].as_str().ok_or("digest")?, 16)?,
                u64::from_str_radix(c["digest"][1].as_str().ok_or("digest")?, 16)?,
            ],
            scanned_at: c["scanned_at"].as_u64().ok_or("invalid time")?,
        });
    }
    compare(&[], &out)?;
    Ok(out)
}

#[derive(Debug, PartialEq)]
pub enum Hint {
    Unknown,
    Changed,
    Hot,
    NoPositiveEvidence,
}
pub fn hint(previous: Option<u64>, current: Option<u64>, probed_at: u64) -> Hint {
    let Some(now) = current else {
        return Hint::Unknown;
    };
    if previous.is_some_and(|p| probed_at.saturating_sub(p) < 2) {
        Hint::Hot
    } else if previous.is_none_or(|p| now > p) {
        Hint::Changed
    } else {
        Hint::NoPositiveEvidence
    }
}
pub async fn table_hints(conn: &mut Conn, schema: &str) -> Result<Vec<(String, Option<u64>)>> {
    Ok(conn.exec("SELECT TABLE_NAME, UNIX_TIMESTAMP(UPDATE_TIME) FROM information_schema.TABLES WHERE TABLE_SCHEMA = ?", (schema,)).await?)
}

#[derive(Clone, Copy, Debug)]
pub struct Window {
    start: u16,
    end: u16,
}
impl Window {
    pub fn parse(text: &str) -> Result<Self> {
        fn minute(s: &str) -> Result<u16> {
            let (h, m) = s.split_once(':').ok_or("invalid window")?;
            let h: u16 = h.parse()?;
            let m: u16 = m.parse()?;
            if h > 23 || m > 59 {
                return Err("invalid window".into());
            }
            Ok(h * 60 + m)
        }
        let (s, e) = text.split_once('-').ok_or("invalid window")?;
        Ok(Self {
            start: minute(s)?,
            end: minute(e)?,
        })
    }
    pub fn contains(self, minute: u16) -> bool {
        minute < 1440
            && if self.start == self.end {
                true
            } else if self.start < self.end {
                minute >= self.start && minute < self.end
            } else {
                minute >= self.start || minute < self.end
            }
    }
}
#[derive(Debug)]
pub struct Budget {
    pub sleep_factor: f64,
    pub max_threads: u64,
    pub client_timeout: Duration,
    pub max_execution_ms: u32,
    ewma: Option<f64>,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            sleep_factor: 3.0,
            max_threads: 8,
            client_timeout: Duration::from_secs(10),
            max_execution_ms: 5000,
            ewma: None,
        }
    }
}
impl Budget {
    pub fn rest(&self, elapsed: Duration) -> Duration {
        elapsed.mul_f64(self.sleep_factor.max(0.0))
    }
    pub fn adapt(&mut self, rows: u64, seconds: f64) -> u64 {
        let rate = rows as f64 / seconds.max(0.001);
        let rate = self.ewma.map_or(rate, |old| old * 0.75 + rate * 0.25);
        self.ewma = Some(rate);
        (rate * 0.5).clamp(1.0, 100_000.0) as u64
    }
}
pub fn full_read_due(last: Option<u64>, now: u64, interval: u64) -> bool {
    last.is_none_or(|t| now.saturating_sub(t) >= interval)
}

pub struct Scanner {
    conn: Conn,
    pub budget: Budget,
    pub paused: Duration,
    pub window: Window,
}
impl Scanner {
    pub fn new(conn: Conn, budget: Budget, window: Window) -> Self {
        Self {
            conn,
            budget,
            paused: Duration::ZERO,
            window,
        }
    }
    pub fn into_connection(self) -> Conn {
        self.conn
    }
    async fn ready(&mut self) -> Result<()> {
        loop {
            let start = Instant::now();
            let minute: Option<u16> = tokio::time::timeout(
                self.budget.client_timeout,
                self.conn.query_first("SELECT HOUR(NOW())*60+MINUTE(NOW())"),
            )
            .await??;
            if !self.window.contains(minute.ok_or("missing clock")?) {
                return Err("OUTSIDE_EXECUTION_WINDOW".into());
            }
            let status: Option<(String, String)> = tokio::time::timeout(
                self.budget.client_timeout,
                self.conn
                    .query_first("SHOW GLOBAL STATUS LIKE 'Threads_running'"),
            )
            .await??;
            if status.ok_or("missing Threads_running")?.1.parse::<u64>()? <= self.budget.max_threads
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            self.paused += start.elapsed();
        }
    }
    pub async fn schema(
        &mut self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<(String, String, Option<String>, String)>> {
        Ok(self.conn.exec("SELECT COLUMN_NAME,COLUMN_TYPE,COLLATION_NAME,IS_NULLABLE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION", (schema,table)).await?)
    }
    pub async fn check_schema(
        &mut self,
        schema: &str,
        table: &str,
        expected: &[(String, String, Option<String>, String)],
    ) -> Result<()> {
        if self.schema(schema, table).await? != expected {
            return Err("SCHEMA_DRIFT".into());
        }
        Ok(())
    }
    pub async fn digest(&mut self, sql: &str, params: Params) -> Result<([u64; 3], Duration)> {
        self.ready().await?;
        let start = Instant::now();
        let mut opts = TxOpts::default();
        opts.with_readonly(true).with_consistent_snapshot(true);
        let mut tx = self.conn.start_transaction(opts).await?;
        let row: Option<(u64, u64, u64)> =
            tokio::time::timeout(self.budget.client_timeout, tx.exec_first(sql, params)).await??;
        tx.commit().await?;
        let elapsed = start.elapsed();
        tokio::time::sleep(self.budget.rest(elapsed)).await;
        let (n, a, b) = row.ok_or("missing digest")?;
        Ok(([n, a, b], elapsed))
    }
    pub async fn rows(
        &mut self,
        schema: &str,
        table: &str,
        columns: &[&str],
        predicate: &str,
        params: Params,
    ) -> Result<Vec<Vec<Value>>> {
        self.ready().await?;
        let cols = columns
            .iter()
            .map(|c| ident(c))
            .collect::<Result<Vec<_>>>()?
            .join(",");
        if cols.is_empty() {
            return Err("empty projection".into());
        }
        let sql = format!(
            "SELECT /*+ MAX_EXECUTION_TIME({}) */ {cols} FROM {}.{} WHERE ({predicate})",
            self.budget.max_execution_ms,
            ident(schema)?,
            ident(table)?
        );
        let start = Instant::now();
        let mut opts = TxOpts::default();
        opts.with_readonly(true).with_consistent_snapshot(true);
        let mut tx = self.conn.start_transaction(opts).await?;
        let rows: Vec<mysql_async::Row> =
            tokio::time::timeout(self.budget.client_timeout, tx.exec(sql, params)).await??;
        tx.commit().await?;
        tokio::time::sleep(self.budget.rest(start.elapsed())).await;
        Ok(rows.into_iter().map(|r| r.unwrap()).collect())
    }
    pub async fn typed_rows(
        &mut self,
        schema: &str,
        table: &str,
        columns: &[(&str, StoreKind, bool)],
        key_indices: &[usize],
        predicate: &str,
        params: Params,
    ) -> Result<Vec<TypedRow>> {
        if key_indices.is_empty() || key_indices.iter().any(|i| *i >= columns.len()) {
            return Err("invalid key projection".into());
        }
        let names = columns.iter().map(|c| c.0).collect::<Vec<_>>();
        let raw = self.rows(schema, table, &names, predicate, params).await?;
        raw.into_iter()
            .map(|row| {
                let key = key_indices.iter().map(|i| row[*i].clone()).collect();
                let mut values = Vec::new();
                for (value, (_, kind, nullable)) in row.into_iter().zip(columns) {
                    let raw = match value {
                        Value::NULL => RawCell::Null,
                        Value::Int(v) => RawCell::Int(v),
                        Value::UInt(v) => RawCell::Uint(v),
                        Value::Float(v) => RawCell::Float(v),
                        Value::Double(v) => RawCell::Double(v),
                        Value::Bytes(v) => RawCell::Bytes(v),
                        Value::Date(year, month, day, hour, minute, second, micros) => {
                            RawCell::Date {
                                year,
                                month,
                                day,
                                hour,
                                minute,
                                second,
                                micros,
                            }
                        }
                        Value::Time(negative, days, hours, minutes, seconds, micros) => {
                            RawCell::Time {
                                negative,
                                days,
                                hours,
                                minutes,
                                seconds,
                                micros,
                            }
                        }
                    };
                    let (cell, reject) = decode_cell(*kind, &raw, *nullable)?;
                    if reject.is_some() {
                        return Err("CELL_REJECT".into());
                    }
                    values.push(cell);
                }
                Ok(TypedRow { key, values })
            })
            .collect()
    }
}
pub fn recent_predicate(column: &str, now: Value, lower: Value) -> Result<(String, Params)> {
    Ok((
        format!("{} >= ? AND {} <= ?", ident(column)?, ident(column)?),
        Params::Positional(vec![lower, now]),
    ))
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedRow {
    pub key: Vec<Value>,
    pub values: Vec<DecodedCell>,
}
pub trait TargetRows {
    fn current_rows(&self, chunk_id: &str) -> Result<Vec<TypedRow>>;
}
#[derive(Debug, PartialEq)]
pub enum Mutation {
    Insert(TypedRow),
    Update(TypedRow),
    Delete {
        key: Vec<Value>,
        tombstone_generation: u64,
    },
}
pub fn reconcile(
    source: &[TypedRow],
    target: &impl TargetRows,
    chunk_id: &str,
    generation: u64,
) -> Result<Vec<Mutation>> {
    let old = target.current_rows(chunk_id)?;
    fn index(rows: &[TypedRow]) -> Result<BTreeMap<Vec<u8>, &TypedRow>> {
        let mut map = BTreeMap::new();
        for row in rows {
            let mut key = Vec::new();
            for value in &row.key {
                match value {
                    Value::Bytes(v) => {
                        key.push(0);
                        key.extend_from_slice(&(v.len() as u64).to_be_bytes());
                        key.extend_from_slice(v);
                    }
                    Value::Int(v) => {
                        key.push(1);
                        key.extend_from_slice(&v.to_be_bytes());
                    }
                    Value::UInt(v) => {
                        key.push(2);
                        key.extend_from_slice(&v.to_be_bytes());
                    }
                    _ => return Err("unsupported row key type".into()),
                }
            }
            if row.key.is_empty() || map.insert(key, row).is_some() {
                return Err("duplicate or empty row identity".into());
            }
        }
        Ok(map)
    }
    let source = index(source)?;
    let old = index(&old)?;
    let mut out = Vec::new();
    for (key, row) in &source {
        match old.get(key) {
            None => out.push(Mutation::Insert((*row).clone())),
            Some(r) if r.values != row.values => out.push(Mutation::Update((*row).clone())),
            _ => (),
        }
    }
    for (key, row) in old {
        if !source.contains_key(&key) {
            out.push(Mutation::Delete {
                key: row.key.clone(),
                tombstone_generation: generation,
            });
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct ScanProgress {
    pub table: String,
    pub previous: Vec<Chunk>,
    pub completed: Vec<Chunk>,
    pub planned: Vec<(String, Json)>,
}
impl ScanProgress {
    pub fn next_pending(&self) -> Option<&(String, Json)> {
        self.planned.get(self.completed.len())
    }
    pub fn checkpoint_applied(&mut self, chunk: Chunk, path: &Path) -> Result<()> {
        let expected = self.next_pending().ok_or("scan already complete")?;
        if expected.0 != chunk.chunk_id || expected.1 != chunk.bound {
            return Err("unexpected checkpoint block".into());
        }
        let mut next = self.clone();
        next.completed.push(chunk);
        save(path, &next.to_json())?;
        *self = next;
        Ok(())
    }
    pub fn changes(&self) -> Result<Changes> {
        if self.next_pending().is_some() {
            return Err("incomplete scan".into());
        }
        compare(&self.previous, &self.completed)
    }
    pub fn to_json(&self) -> Json {
        json!({"version":1,"table":self.table,"previous":self.previous.iter().map(chunk_json).collect::<Vec<_>>(),"chunks":self.completed.iter().map(chunk_json).collect::<Vec<_>>(),"planned":self.planned})
    }
    pub fn load(path: &Path) -> Result<Self> {
        let value: Json = serde_json::from_slice(&std::fs::read(path)?)?;
        let completed = load_chunks(&value)?;
        let previous = load_chunks(&json!({"version":1,"chunks":value["previous"]}))?;
        let planned: Vec<(String, Json)> = serde_json::from_value(value["planned"].clone())?;
        if completed.len() > planned.len()
            || completed
                .iter()
                .zip(&planned)
                .any(|(c, (id, bound))| c.chunk_id != *id || c.bound != *bound)
        {
            return Err("invalid scan progress".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        if planned.iter().any(|(id, _)| !ids.insert(id)) {
            return Err("duplicate planned block".into());
        }
        Ok(Self {
            table: value["table"].as_str().ok_or("missing table")?.into(),
            previous,
            completed,
            planned,
        })
    }
}
