//! Full selected source/target equality, independent of capture and Loader output.
//!
//! Both sides are read in primary-key pages of at most `PAGE_ROWS` rows. Each row
//! is hashed from its normalized cells. The row hashes are summed into key buckets
//! (`2^bits` equal ranges of the source key range). Memory is bounded by one page
//! and the bucket table, not by the table size. A bucket whose count or sum differs
//! is drilled down by key range, and the first differing keys go to
//! `typed-oracle-diff.json`.
use crate::{io::*, mysql::source_sql};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

const PAGE_ROWS: u64 = 4096;
const BUCKET_BITS: u32 = 10;
const BUCKET_BITS_MAX: u32 = 16;
const EXAMPLE_LIMIT: usize = 20;
const DRILL_LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyBuckets {
    lo: i128,
    span: i128,
    bits: u32,
}

impl KeyBuckets {
    fn new(lo: i128, hi: i128, bits: u32) -> Result<Self> {
        require(bits <= BUCKET_BITS_MAX && hi >= lo, "oracle_bucket_bits")?;
        Ok(Self {
            lo,
            span: hi - lo + 1,
            bits,
        })
    }
    fn len(&self) -> usize {
        1usize << self.bits
    }
    fn index(&self, key: i128) -> usize {
        let offset = key - self.lo;
        if offset <= 0 {
            return 0;
        }
        let index = offset as u128 * self.len() as u128 / self.span as u128;
        index.min(self.len() as u128 - 1) as usize
    }
    fn range(&self, index: usize) -> (Option<i128>, Option<i128>) {
        let buckets = self.len() as i128;
        let i = index as i128;
        let start = (index > 0).then(|| self.lo + ceil_div(i * self.span, buckets));
        let end =
            (index + 1 < self.len()).then(|| self.lo + ceil_div((i + 1) * self.span, buckets) - 1);
        (start, end)
    }
}

fn ceil_div(value: i128, divisor: i128) -> i128 {
    (value + divisor - 1) / divisor
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct BucketDigest {
    count: u64,
    sum: [u8; 32],
}

impl BucketDigest {
    fn add(&mut self, bytes: &[u8]) {
        let hash: [u8; 32] = Sha256::digest(bytes).into();
        let mut carry = 0u16;
        for index in (0..32).rev() {
            let total = u16::from(self.sum[index]) + u16::from(hash[index]) + carry;
            self.sum[index] = total as u8;
            carry = total >> 8;
        }
        self.count += 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Source,
    Target,
}

#[derive(Clone, Copy, Default, Debug)]
struct Range {
    start: Option<i128>,
    end: Option<i128>,
    after: Option<i128>,
}

#[derive(Clone, Debug)]
pub struct TableSpec {
    schema: String,
    table: String,
    target_host: String,
    target_database: String,
    target_user: String,
    target_table: String,
    names: Vec<String>,
    kinds: Vec<String>,
    source_exprs: Vec<String>,
    target_exprs: Vec<String>,
    key: usize,
    key_indexes: Vec<usize>,
    key_name: String,
    key_physical: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Row {
    key: i128,
    cells: Vec<Value>,
}

trait Pages {
    fn type_policy(&self) -> Value { Value::Null }
    fn info(&mut self, sql: &str) -> Result<String>;
    fn bounds(&mut self, spec: &TableSpec) -> Result<(i128, i128)>;
    fn fetch(&mut self, side: Side, spec: &TableSpec, range: Range) -> Result<Vec<Row>>;
}

fn normalized(v: &Value, kind: &str) -> Result<Value> {
    if v.is_null() {
        return Ok(Value::Null);
    }
    let s = v.as_str().ok_or("source_cell_text")?;
    let s = if kind == "timestamp" {
        s.strip_suffix("+00:00").or_else(||s.strip_suffix("+00")).or_else(||s.strip_suffix('Z')).unwrap_or(s)
    } else { s };
    Ok(match kind {
        "decimal" => json!(decimal(s)?),
        "double" | "float" => json!(format!("{:.12e}", s.parse::<f64>().map_err(|_| "float_text")?)),
        "json" => serde_json::from_str(s)?,
        "time" => json!(time(s)?),
        "varbinary" | "binary" => json!(s.strip_prefix("\\x").unwrap_or(s).to_ascii_lowercase()),
        "date" | "datetime" | "timestamp" => json!(if s.contains('.') {
            s.replace('T', " ")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        } else {
            s.replace('T', " ")
        }),
        _ => json!(s),
    })
}
fn decimal(s: &str) -> Result<String> {
    let negative = s.starts_with('-');
    let s = s
        .strip_prefix('-')
        .or_else(|| s.strip_prefix('+'))
        .unwrap_or(s);
    let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));
    require(
        !whole.is_empty()
            && whole
                .bytes()
                .chain(fraction.bytes())
                .all(|b| b.is_ascii_digit()),
        "decimal_text",
    )?;
    let whole = whole.trim_start_matches('0');
    let fraction = fraction.trim_end_matches('0');
    let mut result = if whole.is_empty() {
        "0".into()
    } else {
        whole.to_owned()
    };
    if !fraction.is_empty() {
        result.push('.');
        result.push_str(fraction);
    }
    if negative && result != "0" {
        result.insert(0, '-');
    }
    Ok(result)
}
fn time(s: &str) -> Result<i128> {
    let sign = if s.starts_with('-') { -1 } else { 1 };
    let s = s.trim_start_matches('-');
    // PostgreSQL interval textual output preserves negative MySQL hours.
    let s = s.strip_prefix('+').unwrap_or(s);
    let p = s.split(':').collect::<Vec<_>>();
    require(p.len() == 3, "time_text")?;
    let (seconds, fraction) = p[2].split_once('.').unwrap_or((p[2], ""));
    require(
        fraction.len() <= 6 && fraction.bytes().all(|b| b.is_ascii_digit()),
        "time_precision",
    )?;
    let hours = p[0].parse::<i128>()?;
    let minutes = p[1].parse::<i128>()?;
    let seconds = seconds.parse::<i128>()?;
    require(
        (0..60).contains(&minutes) && (0..60).contains(&seconds),
        "time_range",
    )?;
    let micros = format!("{fraction:0<6}").parse::<i128>()?;
    Ok(sign * ((hours * 3600 + minutes * 60 + seconds) * 1_000_000 + micros))
}
fn integer_kind(kind: &str) -> bool {
    matches!(
        kind,
        "tinyint" | "smallint" | "mediumint" | "int" | "bigint"
    )
}

fn key_of(value: &Value) -> Result<i128> {
    value
        .as_str()
        .ok_or("oracle_key_text")?
        .parse::<i128>()
        .map_err(|_| "oracle_key_text".into())
}

fn source_from(spec: &TableSpec) -> String {
    format!("`{}`.`{}`", spec.schema, spec.table)
}

fn page_sql(side: Side, spec: &TableSpec, range: Range) -> String {
    let (func, exprs, from, key) = match side {
        Side::Source => (
            "JSON_ARRAY",
            &spec.source_exprs,
            source_from(spec),
            format!("`{}`", spec.key_name),
        ),
        Side::Target => (
            "json_build_array",
            &spec.target_exprs,
            format!("_oo.\"{}\"", spec.target_table),
            format!("\"{}\"", spec.key_physical),
        ),
    };
    let mut filters = Vec::new();
    if let Some(start) = range.start {
        filters.push(format!("{key} >= {start}"));
    }
    if let Some(after) = range.after {
        filters.push(format!("{key} > {after}"));
    }
    if let Some(end) = range.end {
        filters.push(format!("{key} <= {end}"));
    }
    let filter = if filters.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", filters.join(" AND "))
    };
    format!(
        "SELECT {func}({}) FROM {from}{filter} ORDER BY {key} LIMIT {PAGE_ROWS}",
        join_args(side, exprs)
    )
}

/// PostgreSQL refuses more than 100 function arguments; the target side passes one array so wide tables are readable.
fn join_args(side: Side, exprs: &[String]) -> String {
    match side {
        Side::Source => exprs.join(","),
        Side::Target => format!("VARIADIC ARRAY[{}]", exprs.join(",")),
    }
}

fn parse_page(stdout: &str, spec: &TableSpec) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for line in stdout.lines().filter(|line| !line.is_empty()) {
        let cells: Vec<Value> = serde_json::from_str(line)?;
        require(cells.len() == spec.kinds.len(), "row_width")?;
        let normalized = cells
            .iter()
            .zip(&spec.kinds)
            .map(|(cell, kind)| normalized(cell, kind))
            .collect::<Result<Vec<_>>>()?;
        let key = key_of(&normalized[spec.key])?;
        rows.push(Row {
            key,
            cells: normalized,
        });
    }
    Ok(rows)
}

fn parse_bounds(stdout: &str) -> Result<(i128, i128)> {
    let line = stdout.lines().next().ok_or("oracle_bounds")?;
    let mut pair = line.split('\t');
    let lo = pair.next().ok_or("oracle_bounds")?;
    let hi = pair.next().ok_or("oracle_bounds")?;
    if lo == "NULL" || hi == "NULL" {
        return Ok((0, 0));
    }
    Ok((lo.parse::<i128>()?, hi.parse::<i128>()?))
}

fn bounds_sql(spec: &TableSpec) -> String {
    format!(
        "SELECT CAST(MIN(`{k}`) AS CHAR), CAST(MAX(`{k}`) AS CHAR) FROM {}",
        source_from(spec),
        k = spec.key_name
    )
}

fn table_spec(
    pages: &mut dyn Pages,
    schema: &str,
    table: &str,
    logical: &Value,
    target: &Value,
) -> Result<TableSpec> {
    table_spec_inner(pages, schema, table, logical, target, false)
}

fn table_spec_inner(
    pages: &mut dyn Pages,
    schema: &str,
    table: &str,
    logical: &Value,
    target: &Value,
    full: bool,
) -> Result<TableSpec> {
    let types = pages.info(&format!(
        "SELECT COLUMN_NAME,DATA_TYPE,COLUMN_KEY FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}' ORDER BY ORDINAL_POSITION"
    ))?;
    let columns = array(logical, "columns")?;
    let mut names = Vec::new();
    let mut kinds = Vec::new();
    let mut source_exprs = Vec::new();
    let mut target_exprs = Vec::new();
    let mut key = None;
    let mut key_indexes = Vec::new();
    for line in types.lines() {
        let mut fields = line.split('\t');
        let name = ident(fields.next().ok_or("column")?)?;
        let kind = fields.next().ok_or("kind")?;
        let column_key = fields.next().unwrap_or("");
        let Some(col) = columns.iter().find(|col| {
            col["logical_name"] == name || col["name"] == name || col["logical_column"] == name
        }) else {
            require(column_key != "PRI", "oracle_key_shape")?;
            continue;
        };
        let physical = ident(text(col, "stored_physical")?)?;
        let quoted = format!("`{name}`");
        let mut expression = format!("CAST({quoted} AS CHAR)");
        if ["binary", "varbinary"].contains(&kind) {
            expression = format!("HEX({quoted})");
        }
        if ["date", "datetime"].contains(&kind) {
            expression = format!(
                "CASE WHEN CAST({quoted} AS CHAR) REGEXP '^(0000-[0-9]{{2}}-[0-9]{{2}}|[0-9]{{4}}-00-[0-9]{{2}}|[0-9]{{4}}-[0-9]{{2}}-00)( |$)' THEN NULL ELSE {expression} END"
            );
        }
        let policy = pages.type_policy();
        if policy["strip_text_nul"] == true && column_key != "PRI" && ["char","varchar","text","longtext","mediumtext","tinytext"].contains(&kind) {
            expression = format!("REPLACE({expression},CHAR(0),'')");
        }
        if kind == "timestamp" && policy["zero_date_to_null"] == true {
            expression = format!("CASE WHEN CAST({quoted} AS CHAR) LIKE '0000-%' THEN NULL ELSE {expression} END");
        }
        if column_key == "PRI" {
            require(
                (full || key.is_none())
                    && (integer_kind(kind) || matches!(kind, "varchar" | "char" | "date" | "datetime" | "varbinary" | "binary")),
                "oracle_key_shape",
            )?;
            key_indexes.push(names.len());
            if key.is_none() {
                key = Some((names.len(), physical.to_owned()));
            }
        }
        source_exprs.push(expression);
        target_exprs.push(if kind == "timestamp" {
            format!("(\"{physical}\" AT TIME ZONE 'UTC')::text")
        } else if ["binary", "varbinary"].contains(&kind) {
            format!("encode(\"{physical}\",'hex')")
        } else {
            format!("\"{physical}\"::text")
        });
        names.push(name.to_owned());
        kinds.push(kind.to_owned());
    }
    require(
        !kinds.is_empty() && kinds.len() == columns.len(),
        "oracle_column_coverage",
    )?;
    let (key, key_physical) = if let Some(key) = key {
        key
    } else if full {
        (0, text(&columns[0], "stored_physical")?.to_owned())
    } else {
        return Err("oracle_key_shape".into());
    };
    Ok(TableSpec {
        schema: schema.to_owned(),
        table: table.to_owned(),
        target_host: text(target, "host")?.to_owned(),
        target_database: text(target, "database")?.to_owned(),
        target_user: ident(target["user"].as_str().unwrap_or("oo_admin"))?.to_owned(),
        target_table: ident(text(logical, "stored_physical")?)?.to_owned(),
        key_name: names[key].clone(),
        names,
        kinds,
        source_exprs,
        target_exprs,
        key,
        key_indexes,
        key_physical,
    })
}

#[derive(Default, Debug)]
struct Tally {
    missing_in_target: u64,
    extra_in_target: u64,
    changed: u64,
    examples: Vec<Value>,
}

impl Tally {
    fn note(&mut self, kind: &str, key: i128, column: Option<&str>) {
        match kind {
            "missing_in_target" => self.missing_in_target += 1,
            "extra_in_target" => self.extra_in_target += 1,
            _ => self.changed += 1,
        }
        if self.examples.len() < EXAMPLE_LIMIT {
            self.examples
                .push(json!({"key": key.to_string(), "kind": kind, "column": column}));
        }
    }
}

struct Cursor {
    side: Side,
    range: Range,
    buffer: VecDeque<Row>,
    done: bool,
}

impl Cursor {
    fn new(side: Side, bounds: Range) -> Self {
        Self {
            side,
            range: bounds,
            buffer: VecDeque::new(),
            done: false,
        }
    }

    fn next(&mut self, pages: &mut dyn Pages, spec: &TableSpec) -> Result<Option<Row>> {
        loop {
            if let Some(row) = self.buffer.pop_front() {
                if self.range.end.is_some_and(|end| row.key > end) {
                    self.buffer.clear();
                    self.done = true;
                    return Ok(None);
                }
                return Ok(Some(row));
            }
            if self.done {
                return Ok(None);
            }
            let page = pages.fetch(self.side, spec, self.range)?;
            self.done = (page.len() as u64) < PAGE_ROWS;
            if let Some(last) = page.last() {
                self.range.after = Some(last.key);
            }
            self.buffer.extend(page);
        }
    }
}

fn drill(pages: &mut dyn Pages, spec: &TableSpec, bounds: Range, tally: &mut Tally) -> Result<()> {
    let mut source = Cursor::new(Side::Source, bounds);
    let mut target = Cursor::new(Side::Target, bounds);
    let mut left = source.next(pages, spec)?;
    let mut right = target.next(pages, spec)?;
    loop {
        let order = match (&left, &right) {
            (None, None) => break,
            (Some(a), Some(b)) => a.key.cmp(&b.key),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
        };
        match order {
            std::cmp::Ordering::Less => {
                let Some(a) = left.take() else { break };
                tally.note("missing_in_target", a.key, None);
                left = source.next(pages, spec)?;
            }
            std::cmp::Ordering::Greater => {
                let Some(b) = right.take() else { break };
                tally.note("extra_in_target", b.key, None);
                right = target.next(pages, spec)?;
            }
            std::cmp::Ordering::Equal => {
                let (Some(a), Some(b)) = (left.take(), right.take()) else {
                    break;
                };
                let column = (0..spec.names.len())
                    .find(|&index| a.cells.get(index) != b.cells.get(index))
                    .map(|index| spec.names[index].clone());
                if let Some(column) = column {
                    tally.note("changed", a.key, Some(&column));
                }
                left = source.next(pages, spec)?;
                right = target.next(pages, spec)?;
            }
        }
    }
    Ok(())
}

struct Outcome {
    rows: u64,
    target_rows: u64,
    buckets: usize,
    mismatched: Vec<usize>,
    pages: u64,
    digest: String,
    tally: Tally,
}

fn scan(
    pages: &mut dyn Pages,
    spec: &TableSpec,
    side: Side,
    buckets: &KeyBuckets,
) -> Result<(Vec<BucketDigest>, u64, u64)> {
    let mut digests = vec![BucketDigest::default(); buckets.len()];
    let mut range = Range::default();
    let mut rows = 0u64;
    let mut count = 0u64;
    loop {
        let page = pages.fetch(side, spec, range)?;
        count += 1;
        let last = page.last().map(|row| row.key);
        let full = page.len() as u64 == PAGE_ROWS;
        for row in &page {
            let bytes = serde_json::to_vec(&row.cells)?;
            digests[buckets.index(row.key)].add(&bytes);
            rows += 1;
        }
        if !full {
            break;
        }
        range.after = last;
    }
    Ok((digests, rows, count))
}

fn compare_table(pages: &mut dyn Pages, spec: &TableSpec, bits: u32) -> Result<Outcome> {
    let (lo, hi) = pages.bounds(spec)?;
    let buckets = KeyBuckets::new(lo, hi, bits)?;
    let (source, rows, source_pages) = scan(pages, spec, Side::Source, &buckets)?;
    let (target, target_rows, target_pages) = scan(pages, spec, Side::Target, &buckets)?;
    let mismatched: Vec<usize> = (0..source.len())
        .filter(|&index| source[index] != target[index])
        .collect();
    let mut tally = Tally::default();
    for index in mismatched.iter().take(DRILL_LIMIT) {
        let (start, end) = buckets.range(*index);
        drill(
            pages,
            spec,
            Range {
                start,
                end,
                after: None,
            },
            &mut tally,
        )?;
    }
    let mut hasher = Sha256::new();
    for bucket in &source {
        hasher.update(bucket.count.to_be_bytes());
        hasher.update(bucket.sum);
    }
    let digest = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(Outcome {
        rows,
        target_rows,
        buckets: buckets.len(),
        mismatched,
        pages: source_pages + target_pages,
        digest,
        tally,
    })
}

fn bucket_bits(c: &Value) -> Result<u32> {
    let bits = match c.get("oracle_bucket_bits") {
        None | Some(Value::Null) => BUCKET_BITS,
        Some(value) => value.as_u64().ok_or("oracle_bucket_bits")? as u32,
    };
    require(bits <= BUCKET_BITS_MAX, "oracle_bucket_bits")?;
    Ok(bits)
}

struct CallPages<'a> {
    c: &'a Value,
    calls: &'a mut Calls,
}

fn stdout_files(dir: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut found = BTreeSet::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "stdout") {
            found.insert(path);
        }
    }
    Ok(found)
}

fn discard_new_stdout(dir: &Path, before: &BTreeSet<PathBuf>) -> Result<()> {
    for path in stdout_files(dir)? {
        if !before.contains(&path) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

impl CallPages<'_> {
    fn source(&mut self, sql: &str) -> Result<String> {
        let before = stdout_files(&self.calls.out)?;
        let out = source_sql(self.c, self.calls, sql);
        discard_new_stdout(&self.calls.out, &before)?;
        out
    }

    fn target(&mut self, spec: &TableSpec, sql: &str) -> Result<String> {
        let before = stdout_files(&self.calls.out)?;
        let out = self.calls.docker(
            "typed-target",
            &spec.target_host,
            &strings(&[
                "psql",
                "-U",
                &spec.target_user,
                "-d",
                &spec.target_database,
                "-X",
                "-v",
                "ON_ERROR_STOP=1",
                "-At",
            ]),
            Some(sql.as_bytes()),
        );
        discard_new_stdout(&self.calls.out, &before)?;
        Ok(String::from_utf8(out?)?)
    }
}

impl Pages for CallPages<'_> {
    fn type_policy(&self) -> Value {
        self.c["onboarding"]["full"]["pipeline"]["capture_template"]["type_policy"].clone()
    }
    fn info(&mut self, sql: &str) -> Result<String> {
        self.source(sql)
    }

    fn bounds(&mut self, spec: &TableSpec) -> Result<(i128, i128)> {
        parse_bounds(&self.source(&bounds_sql(spec))?)
    }

    fn fetch(&mut self, side: Side, spec: &TableSpec, range: Range) -> Result<Vec<Row>> {
        let sql = page_sql(side, spec, range);
        let stdout = match side {
            Side::Source => self.source(&sql)?,
            Side::Target => self.target(spec, &sql)?,
        };
        parse_page(&stdout, spec)
    }
}

fn bounded_sql(side: Side, spec: &TableSpec, max_rows: u64) -> String {
    let (func, exprs, from) = match side {
        Side::Source => ("JSON_ARRAY", &spec.source_exprs, source_from(spec)),
        Side::Target => (
            "json_build_array",
            &spec.target_exprs,
            format!("_oo.\"{}\"", spec.target_table),
        ),
    };
    format!(
        "SELECT {func}({}) FROM {from} LIMIT {}",
        join_args(side, exprs),
        max_rows + 1
    )
}

fn compare_bounded_cells(
    spec: &TableSpec,
    source: &str,
    target: &str,
    max_rows: u64,
    max_bytes: usize,
) -> Result<u64> {
    compare_cell_lines(
        &spec.kinds,
        &spec.key_indexes,
        source,
        target,
        max_rows,
        max_bytes,
    )
}

pub fn compare_cell_lines(
    kinds: &[String],
    key_indexes: &[usize],
    source: &str,
    target: &str,
    max_rows: u64,
    max_bytes: usize,
) -> Result<u64> {
    let read = |raw: &str| -> Result<(std::collections::BTreeMap<String, u64>, u64)> {
        require(raw.len() <= max_bytes, "oracle_byte_budget")?;
        let mut rows = std::collections::BTreeMap::new();
        let mut keys = BTreeSet::new();
        let mut count = 0;
        for line in raw.lines().filter(|line| !line.is_empty()) {
            require(count < max_rows, "oracle_row_budget")?;
            let cells: Vec<Value> = serde_json::from_str(line)?;
            require(cells.len() == kinds.len(), "row_width")?;
            let cells = cells
                .iter()
                .zip(kinds)
                .map(|(value, kind)| normalized(value, kind))
                .collect::<Result<Vec<_>>>()?;
            if !key_indexes.is_empty() {
                let key = key_indexes
                    .iter()
                    .map(|index| {
                        cells
                            .get(*index)
                            .cloned()
                            .ok_or_else(|| "oracle_key_shape".into())
                    })
                    .collect::<Result<Vec<_>>>()?;
                require(
                    keys.insert(serde_json::to_string(&key)?),
                    "oracle_duplicate_key",
                )?;
            }
            *rows.entry(serde_json::to_string(&cells)?).or_insert(0) += 1;
            count += 1;
        }
        Ok((rows, count))
    };
    let (source, n) = read(source)?;
    let (target, _) = read(target)?;
    require(source == target, "all_cell_typed_difference")?;
    Ok(n)
}

pub fn compare_full(
    c: &Value,
    catalog: &Value,
    targets: &Value,
    calls: &mut Calls,
) -> Result<Value> {
    let max_rows = c["onboarding"]["audit"]["max_rows"]
        .as_u64()
        .filter(|n| *n > 0 && *n < 100_000)
        .ok_or("oracle_row_budget")?;
    let max_bytes = c["onboarding"]["audit"]["max_bytes"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 67_108_864)
        .ok_or("oracle_byte_budget")? as usize;
    require(
        c["onboarding"]["audit"]["frozen"] == true,
        "oracle_requires_frozen_source",
    )?;
    let mut pages = CallPages { c, calls };
    let mut checked = Vec::new();
    for entry in array(catalog, "entries")? {
        for table in array(entry, "tables")? {
            let schema = ident(text(table, "logical_schema")?)?;
            let name = ident(text(table, "logical_table")?)?;
            let dataset = format!("{schema}.{name}");
            let target = targets.get(&dataset).ok_or("oracle_target_binding")?;
            let spec = table_spec_inner(&mut pages, schema, name, table, target, true)?;
            let source = pages.source(&bounded_sql(Side::Source, &spec, max_rows))?;
            let target = pages.target(&spec, &bounded_sql(Side::Target, &spec, max_rows))?;
            let rows = compare_bounded_cells(&spec, &source, &target, max_rows, max_bytes)?;
            require(table["accepted_rows"] == rows, "oracle_catalog_rows")?;
            checked.push(json!({"dataset":dataset,"rows":rows,"columns":spec.names.len(),"different_cells":0}));
        }
    }
    require(!checked.is_empty(), "empty_collection")?;
    Ok(
        json!({"protocol":"oo.mysql.full-typed-oracle/v1","method":"bounded-frozen-cells/v1","tables":checked,"different_cells":0,"common_source_position":null}),
    )
}

pub fn compare(c: &Value, catalog: &Value, result: &Value, calls: &mut Calls) -> Result<Value> {
    let schema = ident(text(c, "database")?)?;
    let bits = bucket_bits(c)?;
    let before = source_sql(c, calls, "SELECT @@GLOBAL.gtid_executed")?
        .trim()
        .to_owned();
    require(result["l_end"] == before, "oracle_source_changed")?;
    let request = crate::mysql::normalize_c1_request(load(calls.out.join("native/request.json"))?)?;
    let mut checked = Vec::new();
    let mut total = 0u64;
    let mut diffs = Vec::new();
    {
        let mut pages = CallPages { c, calls };
        for (e, n) in array(catalog, "entries")?
            .iter()
            .zip(array(&request, "native_requests")?)
        {
            let t = &array(e, "tables")?[0];
            let table = ident(text(t, "logical_table")?)?;
            let spec = table_spec(&mut pages, schema, table, t, &n["target"])?;
            let outcome = compare_table(&mut pages, &spec, bits)?;
            total += outcome.rows;
            if !outcome.mismatched.is_empty() {
                diffs.push(json!({
                    "dataset": format!("{schema}.{table}"),
                    "mismatched_buckets": outcome.mismatched.len(),
                    "missing_in_target": outcome.tally.missing_in_target,
                    "extra_in_target": outcome.tally.extra_in_target,
                    "changed": outcome.tally.changed,
                    "target_rows": outcome.target_rows,
                    "examples": outcome.tally.examples,
                }));
            }
            checked.push(json!({
                "dataset": format!("{schema}.{table}"),
                "rows": outcome.rows,
                "columns": spec.kinds.len(),
                "different_cells": 0,
                "key_column": spec.key_name,
                "buckets": outcome.buckets,
                "mismatched_buckets": outcome.mismatched.len(),
                "pages": outcome.pages,
                "digest": outcome.digest,
            }));
        }
    }
    if !diffs.is_empty() {
        save(
            calls.out.join("typed-oracle-diff.json"),
            &json!({"protocol": "oo.mysql.full-typed-oracle-diff/v1", "tables": diffs}),
        )?;
    }
    require(diffs.is_empty(), "all_cell_typed_difference")?;
    require(
        before == source_sql(c, calls, "SELECT @@GLOBAL.gtid_executed")?.trim(),
        "oracle_source_changed",
    )?;
    Ok(json!({
        "protocol":"oo.mysql.full-typed-oracle/v1",
        "source_id":result["source_id"],
        "source_generation":result["source_generation"],
        "source_gtid":before,
        "method":"key-bucket/v1",
        "bucket_bits":bits,
        "page_rows":PAGE_ROWS,
        "tables":checked,
        "rows":total,
        "different_cells":0
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell as Counter;

    #[test]
    fn exact_decimal_and_signed_mysql_time() {
        assert_eq!(decimal("-000.0000").unwrap(), "0");
        assert_eq!(
            decimal("99999999999999999999.123400").unwrap(),
            "99999999999999999999.1234"
        );
        assert_eq!(time("-25:10:00.123").unwrap(), -90_600_123_000);
        assert_ne!(
            normalized(&Value::Null, "decimal").unwrap(),
            normalized(&json!("0"), "decimal").unwrap()
        );
    }

    #[test]
    fn bounded_varchar_audit_compares_cells_not_capture_digests() {
        let mut s = spec();
        s.kinds[0] = "varchar".into();
        let a = "[\"网点-A\",\"x\",\"1\"]\n[\"a'\",null,\"2\"]\n";
        let b = "[\"a'\",null,\"2\"]\n[\"网点-A\",\"x\",\"1\"]\n";
        assert_eq!(compare_bounded_cells(&s, a, b, 3, 1024).unwrap(), 2);
        assert!(compare_bounded_cells(&s, a, &b.replace("x", "y"), 3, 1024).is_err());
        assert!(compare_bounded_cells(&s, a, b, 1, 1024).is_err());
        assert!(compare_bounded_cells(&s, a, b, 3, 10).is_err());
        assert!(compare_bounded_cells(&s, a, &(b.to_owned() + b), 5, 1024).is_err());
        assert!(!bounded_sql(Side::Source, &s, 3).contains("OFFSET"));
    }

    fn spec() -> TableSpec {
        TableSpec {
            schema: "logistics".into(),
            table: "t".into(),
            target_host: "pg".into(),
            target_database: "db".into(),
            target_user: "oo_admin".into(),
            target_table: "t".into(),
            names: vec!["id".into(), "a".into(), "b".into()],
            kinds: vec!["bigint".into(), "varchar".into(), "text".into()],
            source_exprs: vec![
                "CAST(`id` AS CHAR)".into(),
                "CAST(`a` AS CHAR)".into(),
                "CAST(`b` AS CHAR)".into(),
            ],
            target_exprs: vec![
                "\"id\"::text".into(),
                "\"a\"::text".into(),
                "\"b\"::text".into(),
            ],
            key: 0,
            key_indexes: vec![0],
            key_name: "id".into(),
            key_physical: "id".into(),
        }
    }

    fn row(key: i128, a: Option<&str>, b: &str) -> Row {
        Row {
            key,
            cells: vec![
                json!(key.to_string()),
                a.map_or(Value::Null, |text| json!(text)),
                json!(b),
            ],
        }
    }

    struct Fake {
        source: Vec<Row>,
        target: Vec<Row>,
        info: String,
        fetches: Counter<u64>,
    }

    impl Fake {
        fn new(source: Vec<Row>, target: Vec<Row>) -> Self {
            Self {
                source,
                target,
                info: "id\tbigint\tPRI\na\tvarchar\t\nb\ttext\t\n".into(),
                fetches: Counter::new(0),
            }
        }
    }

    impl Pages for Fake {
        fn info(&mut self, _sql: &str) -> Result<String> {
            Ok(self.info.clone())
        }

        fn bounds(&mut self, _spec: &TableSpec) -> Result<(i128, i128)> {
            let keys: Vec<i128> = self.source.iter().map(|row| row.key).collect();
            Ok((
                keys.iter().copied().min().unwrap_or(0),
                keys.iter().copied().max().unwrap_or(0),
            ))
        }

        fn fetch(&mut self, side: Side, _spec: &TableSpec, range: Range) -> Result<Vec<Row>> {
            self.fetches.set(self.fetches.get() + 1);
            let rows = match side {
                Side::Source => &self.source,
                Side::Target => &self.target,
            };
            let mut selected: Vec<Row> = rows
                .iter()
                .filter(|row| {
                    range.start.is_none_or(|start| row.key >= start)
                        && range.after.is_none_or(|after| row.key > after)
                        && range.end.is_none_or(|end| row.key <= end)
                })
                .cloned()
                .collect();
            selected.sort_by_key(|row| row.key);
            selected.truncate(PAGE_ROWS as usize);
            Ok(selected)
        }
    }

    fn set_a(rows: &mut [Row], key: i128, value: Value) {
        let row = rows.iter_mut().find(|row| row.key == key).expect("key");
        row.cells[1] = value;
    }

    fn sample(count: i128) -> Vec<Row> {
        (1..=count)
            .map(|key| row(key * 3, Some(&format!("v{key}")), &format!("body {key}")))
            .collect()
    }

    #[test]
    fn key_buckets_partition_and_ranges_agree() {
        let buckets = KeyBuckets::new(-3, 20, 2).unwrap();
        for key in -10..=30i128 {
            let index = buckets.index(key);
            for other in 0..buckets.len() {
                let (start, end) = buckets.range(other);
                let inside = start.is_none_or(|s| key >= s) && end.is_none_or(|e| key <= e);
                assert_eq!(inside, index == other, "key {key} bucket {other}");
            }
        }
        assert!(KeyBuckets::new(0, 10, 17).is_err());
        assert!(KeyBuckets::new(10, 0, 4).is_err());
    }

    #[test]
    fn bucket_digest_sums_are_order_independent_and_carry() {
        let mut forward = BucketDigest::default();
        let mut reverse = BucketDigest::default();
        for item in ["a", "b", "c"] {
            forward.add(item.as_bytes());
        }
        for item in ["c", "b", "a"] {
            reverse.add(item.as_bytes());
        }
        assert_eq!(forward, reverse);
        assert_eq!(forward.count, 3);
        let mut carry = BucketDigest {
            count: 0,
            sum: [0xff; 32],
        };
        carry.add(b"x");
        assert_ne!(carry.sum, [0xff; 32]);
    }

    #[test]
    fn bucket_bits_default_and_bound() {
        assert_eq!(bucket_bits(&json!({})).unwrap(), BUCKET_BITS);
        assert_eq!(bucket_bits(&json!({"oracle_bucket_bits": 4})).unwrap(), 4);
        assert!(bucket_bits(&json!({"oracle_bucket_bits": 17})).is_err());
    }

    #[test]
    fn page_sql_is_keyset_on_both_sides_and_bounded() {
        let spec = spec();
        let range = Range {
            start: Some(10),
            end: Some(99),
            after: Some(20),
        };
        let source = page_sql(Side::Source, &spec, range);
        assert!(
            source.starts_with("SELECT JSON_ARRAY(CAST(`id` AS CHAR),"),
            "{source}"
        );
        assert!(source.contains("FROM `logistics`.`t` WHERE `id` >= 10 AND `id` > 20 AND `id` <= 99 ORDER BY `id` LIMIT 4096"), "{source}");
        let target = page_sql(Side::Target, &spec, Range::default());
        assert!(
            target.contains("FROM _oo.\"t\" ORDER BY \"id\" LIMIT 4096"),
            "{target}"
        );
        assert!(target.starts_with("SELECT json_build_array("));
    }

    #[test]
    fn table_spec_reads_exactly_one_integer_primary_key() {
        let logical = json!({"columns": [
            {"logical_name": "id", "name": "id", "stored_physical": "id"},
            {"logical_name": "a", "name": "a", "stored_physical": "a"},
            {"logical_name": "b", "name": "b", "stored_physical": "b"},
        ], "stored_physical": "t"});
        let target = json!({"host": "pg", "database": "db"});
        let mut fake = Fake::new(vec![], vec![]);
        let ok = table_spec(&mut fake, "logistics", "t", &logical, &target).unwrap();
        assert_eq!((ok.key, ok.key_name.as_str()), (0, "id"));

        fake.info = "id\tbigint\t\na\tvarchar\tPRI\n".into();
        assert!(table_spec(&mut fake, "logistics", "t", &logical, &target).is_err());
        fake.info = "id\tbigint\tPRI\na\tvarchar\tPRI\n".into();
        assert!(table_spec(&mut fake, "logistics", "t", &logical, &target).is_err());
        fake.info = "id\tbigint\tPRI\na\tvarchar\t\nb\ttext\t\n".into();
        let short = json!({"columns": [
            {"logical_name": "id", "name": "id", "stored_physical": "id"},
            {"logical_name": "a", "name": "a", "stored_physical": "a"},
            {"logical_name": "b", "name": "b", "stored_physical": "b"},
        ], "stored_physical": "t"});
        fake.info = "id\tbigint\tPRI\na\tvarchar\t\n".into();
        assert!(table_spec(&mut fake, "logistics", "t", &short, &target).is_err());
    }

    #[test]
    fn full_table_spec_accepts_a_single_varchar_key_but_not_a_partial_composite() {
        let logical = json!({"stored_physical":"pt_sealed","columns":[{"logical_name":"code","stored_physical":"pc_code"},{"logical_name":"name","stored_physical":"pc_name"}]});
        let target = json!({"host":"pg","database":"db","user":"fixture_admin"});
        let mut fake = Fake::new(vec![], vec![]);
        fake.info = "code\tvarchar\tPRI\nname\tvarchar\t\n".into();
        let spec = table_spec(&mut fake, "generic", "departments", &logical, &target).unwrap();
        assert_eq!(spec.key_name, "code");
        assert_eq!(spec.target_user, "fixture_admin");
        fake.info.push_str("unselected_key\tvarchar\tPRI\n");
        assert!(table_spec(&mut fake, "generic", "departments", &logical, &target).is_err());
    }

    #[test]
    fn parse_page_normalizes_cells_and_reads_the_key() {
        let spec = spec();
        let rows = parse_page(
            "[\"7\",\"2026-01-02 03:04:05.000000\",null]\n[\"9\",\"x\",\"\"]\n",
            &TableSpec {
                kinds: vec!["bigint".into(), "datetime".into(), "text".into()],
                ..spec
            },
        )
        .unwrap();
        assert_eq!(rows[0].key, 7);
        assert_eq!(rows[0].cells[1], json!("2026-01-02 03:04:05"));
        assert_eq!(rows[0].cells[2], Value::Null);
        assert_eq!(rows[1].cells[2], json!(""));
        assert_eq!(parse_bounds("NULL\tNULL\n").unwrap(), (0, 0));
        assert_eq!(parse_bounds("3\t9\n").unwrap(), (3, 9));
    }

    #[test]
    fn floating_text_is_compared_by_value_not_by_spelling() {
        let n = |v: &str, k: &str| normalized(&json!(v), k).unwrap();
        assert_eq!(n("1.0", "double"), n("1", "double"));
        assert_eq!(n("121.123456", "double"), n("121.12345600000001", "double"));
        assert_ne!(n("0.5", "double"), n("0", "double"));
        assert_ne!(n("121.123456", "float"), n("121.123457", "float"));
    }

    #[test]
    fn identical_tables_match_and_read_only_pages() {
        let spec = spec();
        let mut fake = Fake::new(sample(30), sample(30));
        let outcome = compare_table(&mut fake, &spec, 4).unwrap();
        assert!(outcome.mismatched.is_empty());
        assert_eq!((outcome.rows, outcome.target_rows), (30, 30));
        assert!(outcome.tally.examples.is_empty());
        assert_eq!(outcome.pages, fake.fetches.get());
    }

    #[test]
    fn a_changed_cell_is_localized_to_its_key_and_column() {
        let spec = spec();
        let mut target = sample(30);
        set_a(&mut target, 51, json!("changed"));
        let mut fake = Fake::new(sample(30), target);
        let outcome = compare_table(&mut fake, &spec, 4).unwrap();
        assert_eq!(outcome.mismatched.len(), 1);
        assert_eq!(outcome.tally.changed, 1);
        assert_eq!(
            outcome.tally.examples,
            vec![json!({"key": "51", "kind": "changed", "column": "a"})]
        );
    }

    #[test]
    fn missing_extra_and_null_versus_empty_are_reported_by_key() {
        let spec = spec();
        let mut source = sample(30);
        set_a(&mut source, 27, Value::Null);
        let mut target = sample(30);
        target.retain(|row| row.key != 15);
        target.push(row(99, Some("extra"), "body"));
        set_a(&mut target, 27, json!(""));
        let mut fake = Fake::new(source, target);
        let outcome = compare_table(&mut fake, &spec, 4).unwrap();
        assert_eq!(
            (
                outcome.tally.missing_in_target,
                outcome.tally.extra_in_target,
                outcome.tally.changed
            ),
            (1, 1, 1)
        );
        assert_eq!(
            outcome.tally.examples,
            vec![
                json!({"key": "15", "kind": "missing_in_target", "column": null}),
                json!({"key": "27", "kind": "changed", "column": "a"}),
                json!({"key": "99", "kind": "extra_in_target", "column": null}),
            ]
        );
    }

    #[test]
    fn large_tables_are_read_in_bounded_pages() {
        let spec = spec();
        let mut fake = Fake::new(sample(9000), sample(9000));
        let outcome = compare_table(&mut fake, &spec, 3).unwrap();
        assert!(outcome.mismatched.is_empty());
        assert_eq!(outcome.rows, 9000);
        assert_eq!(outcome.pages, 6);
        assert_eq!(fake.fetches.get(), 6);
    }

    #[test]
    fn only_new_stdout_files_are_discarded() {
        let dir = std::env::temp_dir().join(format!("oo-cb-t11-audit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("0001-profile.stdout"), b"kept").unwrap();
        let before = stdout_files(&dir).unwrap();
        fs::write(dir.join("0002-typed-target.stdout"), b"page").unwrap();
        discard_new_stdout(&dir, &before).unwrap();
        assert!(dir.join("0001-profile.stdout").exists());
        assert!(!dir.join("0002-typed-target.stdout").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    struct ExecPages {
        mysql: String,
        pg: String,
        mysql_db: String,
        pg_db: String,
    }

    fn exec_stdin(args: &[&str], input: Option<&str>) -> Result<String> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("docker")
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(text) = input {
            child
                .stdin
                .take()
                .ok_or("stdin")?
                .write_all(text.as_bytes())?;
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(format!(
                "docker_exec: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    impl ExecPages {
        fn mysql_admin(&self, sql: &str) -> Result<String> {
            exec_stdin(
                &[
                    "exec",
                    "-i",
                    &self.mysql,
                    "mysql",
                    "-uroot",
                    "-N",
                    "-B",
                    "--raw",
                ],
                Some(sql),
            )
        }
        fn mysql_sql(&self, sql: &str) -> Result<String> {
            exec_stdin(
                &[
                    "exec",
                    "-i",
                    &self.mysql,
                    "mysql",
                    "-uroot",
                    "-N",
                    "-B",
                    "--raw",
                    "-D",
                    &self.mysql_db,
                ],
                Some(sql),
            )
        }
        fn pg_sql(&self, sql: &str) -> Result<String> {
            exec_stdin(
                &[
                    "exec",
                    "-i",
                    &self.pg,
                    "psql",
                    "-U",
                    "postgres",
                    "-d",
                    &self.pg_db,
                    "-At",
                    "-v",
                    "ON_ERROR_STOP=1",
                ],
                Some(sql),
            )
        }
    }

    impl Pages for ExecPages {
        fn info(&mut self, sql: &str) -> Result<String> {
            self.mysql_sql(sql)
        }
        fn bounds(&mut self, spec: &TableSpec) -> Result<(i128, i128)> {
            parse_bounds(&self.mysql_sql(&bounds_sql(spec))?)
        }
        fn fetch(&mut self, side: Side, spec: &TableSpec, range: Range) -> Result<Vec<Row>> {
            let sql = page_sql(side, spec, range);
            let stdout = match side {
                Side::Source => self.mysql_sql(&sql)?,
                Side::Target => self.pg_sql(&sql)?,
            };
            parse_page(&stdout, spec)
        }
    }

    fn live_logical(columns: &[(&str, &str)]) -> Value {
        json!({
            "stored_physical": "t11_main",
            "columns": columns
                .iter()
                .map(|(name, _)| json!({"logical_name": name, "name": name, "stored_physical": name}))
                .collect::<Vec<_>>(),
        })
    }

    #[test]
    #[ignore = "requires OO_CB_T11_SO_MYSQL and OO_CB_T11_SO_PG (owned containers) and a docker CLI"]
    fn live_key_buckets_detect_each_injected_difference() {
        let mysql = std::env::var("OO_CB_T11_SO_MYSQL").expect("OO_CB_T11_SO_MYSQL");
        let pg = std::env::var("OO_CB_T11_SO_PG").expect("OO_CB_T11_SO_PG");
        let mut pages = ExecPages {
            mysql,
            pg,
            mysql_db: "oo_cb_t11_so".into(),
            pg_db: "oo".into(),
        };
        pages
            .mysql_admin("DROP DATABASE IF EXISTS oo_cb_t11_so; CREATE DATABASE oo_cb_t11_so")
            .unwrap();
        pages
            .mysql_sql("CREATE TABLE t_main (id BIGINT NOT NULL PRIMARY KEY, note TEXT NULL, amount DECIMAL(38,4) NULL, js JSON NULL) DEFAULT CHARSET=utf8mb4")
            .unwrap();
        pages
            .pg_sql("DROP TABLE IF EXISTS _oo.t11_main; CREATE SCHEMA IF NOT EXISTS _oo; CREATE TABLE _oo.t11_main (id int8 PRIMARY KEY, note text, amount numeric(38,4), js json)")
            .unwrap();
        let mut my_values = Vec::new();
        let mut pg_values = Vec::new();
        for id in 1..=3000i64 {
            let note = if id % 5 == 0 {
                "NULL".to_string()
            } else {
                format!("'note {id}'")
            };
            let amount = if id % 7 == 0 {
                "NULL".to_string()
            } else {
                format!("'{id}.5000'")
            };
            let js = format!("'{{\"a\": [1, 2], \"b\": {id}}}'");
            my_values.push(format!("({id},{note},{amount},{js})"));
            pg_values.push(format!("({id},{note},{amount},{js})"));
        }
        pages
            .mysql_sql(&format!(
                "INSERT INTO t_main (id,note,amount,js) VALUES {}",
                my_values.join(",")
            ))
            .unwrap();
        pages
            .pg_sql(&format!(
                "INSERT INTO _oo.t11_main (id,note,amount,js) VALUES {}",
                pg_values.join(",")
            ))
            .unwrap();

        let logical = live_logical(&[
            ("id", "bigint"),
            ("note", "text"),
            ("amount", "decimal"),
            ("js", "json"),
        ]);
        let target = json!({"host": "unused", "database": "oo"});
        let spec = table_spec(&mut pages, "oo_cb_t11_so", "t_main", &logical, &target).unwrap();
        assert_eq!(spec.key_name, "id");

        let equal = compare_table(&mut pages, &spec, 6).unwrap();
        assert!(
            equal.mismatched.is_empty(),
            "equal tables: {:?}",
            equal.tally.examples
        );
        assert_eq!((equal.rows, equal.target_rows), (3000, 3000));

        let cases: [(&str, &str, &str, Value); 4] = [
            (
                "changed text cell",
                "UPDATE _oo.t11_main SET note = 'changed' WHERE id = 1234",
                "UPDATE _oo.t11_main SET note = 'note 1234' WHERE id = 1234",
                json!({"key": "1234", "kind": "changed", "column": "note"}),
            ),
            (
                "missing row",
                "DELETE FROM _oo.t11_main WHERE id = 2000",
                "INSERT INTO _oo.t11_main (id,note,amount,js) VALUES (2000,NULL,'2000.5000','{\"a\": [1, 2], \"b\": 2000}')",
                json!({"key": "2000", "kind": "missing_in_target", "column": null}),
            ),
            (
                "extra row",
                "INSERT INTO _oo.t11_main (id,note,amount,js) VALUES (3001,'x',1,'{\"a\": [1, 2], \"b\": 3001}')",
                "DELETE FROM _oo.t11_main WHERE id = 3001",
                json!({"key": "3001", "kind": "extra_in_target", "column": null}),
            ),
            (
                "NULL in the source, empty string in the target",
                "UPDATE _oo.t11_main SET note = '' WHERE id = 5",
                "UPDATE _oo.t11_main SET note = NULL WHERE id = 5",
                json!({"key": "5", "kind": "changed", "column": "note"}),
            ),
        ];
        for (label, mutation, revert, expected) in cases {
            pages.pg_sql(mutation).unwrap();
            let report = compare_table(&mut pages, &spec, 6).unwrap();
            pages.pg_sql(revert).unwrap();
            assert_eq!(report.tally.examples, vec![expected], "{label}");
        }
        let restored = compare_table(&mut pages, &spec, 6).unwrap();
        assert!(
            restored.mismatched.is_empty(),
            "revert restores equal tables"
        );
    }
}

#[cfg(test)]
mod multipart_full_cells {
    use super::*;
    #[test]
    fn live_chunk_summary_never_erases_tolerated_drift() {
        let report = chunk_summary(vec![
            json!({"unresolved_chunks":[1, 3], "live_source_post_snapshot_rows":7}),
            json!({"unresolved_chunks":[], "live_source_post_snapshot_rows":2}),
        ]);
        assert_eq!(report["unresolved_chunks"], 2);
        assert_eq!(report["live_source_post_snapshot_rows"], 9);
        assert!(report["different_cells"].is_null());
        let post_only = chunk_summary(vec![json!({"unresolved_chunks":[], "live_source_post_snapshot_rows":1})]);
        assert!(post_only["different_cells"].is_null());
        let frozen = chunk_summary(vec![json!({"unresolved_chunks":[], "live_source_post_snapshot_rows":0})]);
        assert_eq!(frozen["different_cells"], 0);
        assert_eq!(frozen["unresolved_chunks"], 0);
    }

    #[test]
    fn composite_keys_and_unkeyed_duplicate_rows_compare_without_integer_ordering() {
        let kinds = vec!["varchar".to_string(), "int".to_string()];
        let a = "[\"a\",\"1\"]\n[\"a\",\"2\"]\n";
        let b = "[\"a\",\"2\"]\n[\"a\",\"1\"]\n";
        assert_eq!(
            compare_cell_lines(&kinds, &[0, 1], a, b, 10, 4096).unwrap(),
            2
        );
        let repeated = "[\"a\",\"1\"]\n[\"a\",\"1\"]\n";
        assert_eq!(
            compare_cell_lines(&kinds, &[], repeated, repeated, 10, 4096).unwrap(),
            2
        );
        assert!(compare_cell_lines(&kinds, &[0, 1], repeated, repeated, 10, 4096).is_err());
        assert!(compare_cell_lines(&kinds, &[], repeated, a, 10, 4096).is_err());
    }
}

pub fn compare_chunks(
    c: &Value,
    catalog: &Value,
    targets: &Value,
    calls: &mut Calls,
    request: &Value,
) -> Result<Value> {
    let scope = &request["multipart_scope"];
    let directory = Path::new(text(request, "arrow_path")?)
        .parent()
        .ok_or("part_path")?;
    let keys = load(directory.join("chunk-binding.json"))?;
    let key_columns = keys["keys"].as_array().ok_or("chunk_keys")?;
    let mut pages = CallPages { c, calls };
    let mut checked = Vec::new();
    let row_limit = 100000;
    let byte_limit = c["onboarding"]["audit"]["max_bytes"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 67_108_864)
        .ok_or("oracle_byte_budget")? as usize;
    for entry in array(catalog, "entries")? {
        for table in array(entry, "tables")? {
            let schema = ident(text(table, "logical_schema")?)?;
            let name = ident(text(table, "logical_table")?)?;
            let dataset = format!("{schema}.{name}");
            let spec = table_spec_inner(
                &mut pages,
                schema,
                name,
                table,
                targets.get(&dataset).ok_or("oracle_target_binding")?,
                true,
            )?;
            let mut total = 0u64;
            let mut unresolved = Vec::new();
            let mut differences = Vec::new();
            for (index, chunk) in array(scope, "chunks")?.iter().enumerate() {
                let (predicate, _) = chunk_predicate(key_columns, &chunk["lo"], &chunk["hi"])?;
                let source_sql = format!(
                    "SELECT JSON_ARRAY({}) FROM `{schema}`.`{name}`{predicate} LIMIT {}",
                    spec.source_exprs.join(","),
                    row_limit + 1
                );
                let source = pages.source(&source_sql)?;
                let path = directory.join(format!("{index}.arrow"));
                let before = stdout_files(&pages.calls.out)?;
                require(
                    std::fs::metadata(&path)?.len() <= 33554432,
                    "oracle_part_budget",
                )?;
                require(
                    digest(&path)? == scope["parts"][index]["arrow_sha256"],
                    "oracle_capture_part_changed",
                )?;
                let raw = crate::full_pipeline::runtime(
                    c,
                    pages.calls,
                    "oracle-arrow-cells",
                    &strings(&[
                        text(&c["onboarding"]["full"]["pipeline"]["driver"], "path")?,
                        "arrow-cells",
                        "--arrow",
                        path.to_str().ok_or("part_path")?,
                    ]),
                    None,
                )?;
                discard_new_stdout(&pages.calls.out, &before)?;
                let captured = String::from_utf8(raw)?;
                let positions = spec
                    .names
                    .iter()
                    .map(|name| {
                        array(table, "columns")?
                            .iter()
                            .position(|column| column["logical_name"] == *name)
                            .ok_or_else(|| "oracle_column_coverage".into())
                    })
                    .collect::<Result<Vec<_>>>()?;
                let captured = captured
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(|line| -> Result<String> {
                        let cells: Vec<Value> = serde_json::from_str(line)?;
                        Ok(serde_json::to_string(
                            &positions
                                .iter()
                                .map(|i| cells[*i].clone())
                                .collect::<Vec<_>>(),
                        )?)
                    })
                    .collect::<Result<Vec<_>>>()?
                    .join("\n");
                if compare_cell_lines(
                    &spec.kinds,
                    &spec.key_indexes,
                    &source,
                    &captured,
                    row_limit,
                    byte_limit,
                )
                .is_err()
                {
                    unresolved.push(index);
                    continue;
                }
                let members = captured
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(|line| -> Result<Value> {
                        let row: Vec<Value> = serde_json::from_str(line)?;
                        Ok(json!(
                            spec.key_indexes
                                .iter()
                                .map(|i| row[*i].clone())
                                .collect::<Vec<_>>()
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let columns = positions
                    .iter()
                    .map(|i| text(&table["columns"][*i], "stored_physical").map(str::to_owned))
                    .collect::<Result<Vec<_>>>()?;
                let target_keys = spec
                    .key_indexes
                    .iter()
                    .map(|i| columns[*i].clone())
                    .collect::<Vec<_>>();
                let quoted = |values: &[String]| {
                    format!(
                        "ARRAY[{}]::text[]",
                        values
                            .iter()
                            .map(|v| format!("'{}'", v.replace('\'', "''")))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                };
                let member_json = serde_json::to_vec(&members)?
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let sql = format!(
                    "SELECT cells FROM _oo.c5_multipart_cells('{}',{},{},convert_from(decode('{}','hex'),'UTF8')::jsonb) AS r(cells);",
                    spec.target_table,
                    quoted(&columns),
                    quoted(&target_keys),
                    member_json
                );
                let target = pages.target(&spec, &sql)?;
                match compare_cell_lines(
                    &spec.kinds,
                    &spec.key_indexes,
                    &source,
                    &target,
                    row_limit,
                    byte_limit,
                ) {
                    Ok(rows) => total += rows,
                    Err(_) => differences.push(index),
                }
            }
            let mut post_snapshot: u64 = 0;
            if let Some(last) = array(scope, "chunks")?.last() {
                let (tail, _) = chunk_predicate(key_columns, &last["hi"], &Value::Null)?;
                let rows = pages.source(&format!(
                    "SELECT JSON_ARRAY(COUNT(*)) FROM `{schema}`.`{name}`{tail}"
                ))?;
                let count: Vec<Value> = serde_json::from_str(rows.trim())?;
                if count[0].as_u64() != Some(0) {
                    if c["onboarding"]["audit"]["allow_post_snapshot_rows"] == true {
                        post_snapshot = count[0].as_u64().unwrap_or(0);
                    } else {
                        unresolved.push(array(scope, "chunks")?.len());
                    }
                }
            }
            let report = json!({"dataset":dataset,"rows":total,"columns":spec.names.len(),"different_cells":if differences.is_empty() && unresolved.is_empty(){json!(0)}else{json!(null)},"different_chunks":differences,"unresolved_chunks":unresolved,"live_source_post_snapshot_rows":post_snapshot});
            save(
                pages
                    .calls
                    .out
                    .join(format!("chunk-oracle-{}-{}.json", schema, name)),
                &report,
            )?;
            let tolerated = c["onboarding"]["audit"]["allow_unresolved_chunk_fraction"]
                .as_f64()
                .filter(|f| *f > 0.0 && *f < 1.0)
                .is_some_and(|f| {
                    let chunks = array(scope, "chunks").map(|v| v.len()).unwrap_or(0);
                    chunks > 0 && (unresolved.len() as f64) <= f * chunks as f64
                });
            require(unresolved.is_empty() || tolerated, "oracle_source_chunks_unresolved")?;
            require(differences.is_empty(), "all_cell_typed_difference")?;
            require(table["accepted_rows"] == total, "oracle_catalog_rows")?;
            let actual = pages.target(
                &spec,
                &format!("SELECT COUNT(*) FROM _oo.\"{}\";", spec.target_table),
            )?;
            require(
                actual.trim().parse::<u64>()? == total,
                "oracle_target_extra_rows",
            )?;
            checked.push(report);
        }
    }
    Ok(chunk_summary(checked))
}

fn chunk_summary(checked: Vec<Value>) -> Value {
    let unresolved: usize = checked.iter()
        .filter_map(|table| table["unresolved_chunks"].as_array())
        .map(Vec::len).sum();
    let post_snapshot: u64 = checked.iter()
        .filter_map(|table| table["live_source_post_snapshot_rows"].as_u64()).sum();
    json!({"protocol":"oo.mysql.full-typed-oracle/v1","method":"bounded-chunk-cells/v1",
        "tables":checked,"different_cells":if unresolved == 0 && post_snapshot == 0 { json!(0) } else { Value::Null },
        "unresolved_chunks":unresolved,"live_source_post_snapshot_rows":post_snapshot,
        "common_source_position":null})
}

fn chunk_predicate(columns: &[Value], lo: &Value, hi: &Value) -> Result<(String, Vec<String>)> {
    let mut clauses = Vec::new();
    for (tuple, lower) in [(lo, true), (hi, false)] {
        if tuple.is_null() {
            continue;
        }
        let tuple = tuple.as_array().ok_or("chunk_bound")?;
        require(tuple.len() == columns.len(), "chunk_bound")?;
        let mut alternatives = Vec::new();
        for i in 0..columns.len() {
            let mut terms = Vec::new();
            for j in 0..=i {
                let name = ident(text(&columns[j], "name")?)?;
                let value = &tuple[j];
                let literal = if let Some(n) = value.get("Int") {
                    let raw = n.to_string();
                    require(
                        raw.bytes().all(|b| b.is_ascii_digit() || b == b'-'),
                        "chunk_integer",
                    )?;
                    raw
                } else {
                    let (bytes, is_text) = if let Some(bytes) = value.get("Text") {
                        (bytes, true)
                    } else {
                        (value.get("Bytes").ok_or("chunk_value")?, false)
                    };
                    let hex = bytes
                        .as_array()
                        .ok_or("chunk_bytes")?
                        .iter()
                        .map(|v| {
                            v.as_u64()
                                .and_then(|n| u8::try_from(n).ok())
                                .map(|b| format!("{b:02x}"))
                                .ok_or_else(|| "chunk_bytes".into())
                        })
                        .collect::<Result<Vec<_>>>()?
                        .join("");
                    if is_text {
                        let collation = ident(text(&columns[j], "collation")?)?;
                        let charset = ident(collation.split('_').next().ok_or("chunk_collation")?)?;
                        format!(
                            "CONVERT(CONVERT(X'{hex}' USING utf8mb4) USING {charset}) COLLATE {collation}"
                        )
                    } else {
                        format!("X'{hex}'")
                    }
                };
                let op = if j < i {
                    "="
                } else if lower {
                    ">"
                } else if i + 1 == columns.len() {
                    "<="
                } else {
                    "<"
                };
                terms.push(format!("`{name}` {op} {literal}"));
            }
            alternatives.push(format!("({})", terms.join(" AND ")));
        }
        clauses.push(format!("({})", alternatives.join(" OR ")));
    }
    Ok((
        if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        },
        Vec::new(),
    ))
}
