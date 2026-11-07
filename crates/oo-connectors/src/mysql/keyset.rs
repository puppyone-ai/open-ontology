//! Key-shape detection, keyset SQL builders and the chunk planner (S1-C-01).
//!
//! Bounds are `[lo, hi)`. Every comparison is SQL with positional parameters
//! the server evaluates under the key column's own collation: no
//! `COLLATE ..._bin`, no `BINARY`, and no client-side string ordering or
//! sorting. Composite keys are rewritten as equality-prefix disjunctions
//! `a > ? OR (a = ? AND b > ?)` instead of row-value comparisons, and every
//! read names its index with `FORCE INDEX` so `EXPLAIN` can verify it.

use crate::SourceError;
use crate::engine::{KeyShape, ReadBudget};

use super::mysql_ident;

/// Key column type as the planner needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Int,
    Decimal,
    Text,
    Bytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyCol {
    pub name: String,
    pub kind: KeyKind,
    /// Descriptive only. It is never written into SQL.
    pub collation: Option<String>,
}

/// One row of [`KEY_SHAPE_SQL`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexColumn {
    pub index: String,
    pub seq: u32,
    pub column: String,
    pub non_unique: bool,
    pub sub_part: bool,
    pub nullable: bool,
    pub data_type: String,
    pub column_type: String,
    pub collation: Option<String>,
}

/// Unique-index columns of one table, in index then column order. Parameters:
/// schema, table.
pub const KEY_SHAPE_SQL: &str = "SELECT s.INDEX_NAME, s.SEQ_IN_INDEX, s.COLUMN_NAME, s.NON_UNIQUE, s.SUB_PART IS NOT NULL, c.IS_NULLABLE = 'YES', c.DATA_TYPE, c.COLUMN_TYPE, c.COLLATION_NAME FROM information_schema.STATISTICS s JOIN information_schema.COLUMNS c ON c.TABLE_SCHEMA = s.TABLE_SCHEMA AND c.TABLE_NAME = s.TABLE_NAME AND c.COLUMN_NAME = s.COLUMN_NAME WHERE s.TABLE_SCHEMA = ? AND s.TABLE_NAME = ? AND s.NON_UNIQUE = 0 ORDER BY s.INDEX_NAME, s.SEQ_IN_INDEX";

fn key_kind(data_type: &str, column_type: &str) -> Option<KeyKind> {
    match data_type.to_ascii_lowercase().as_str() {
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" => Some(KeyKind::Int),
        "decimal" | "numeric" if column_type.ends_with(",0)") || !column_type.contains(',') => {
            Some(KeyKind::Decimal)
        }
        "char" | "varchar" => Some(KeyKind::Text),
        "binary" | "varbinary" => Some(KeyKind::Bytes),
        _ => None,
    }
}

/// The key the planner uses and its shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChosenKey {
    pub shape: KeyShape,
    /// Index name for `FORCE INDEX`; empty for [`KeyShape::NoKey`].
    pub index: String,
    pub columns: Vec<KeyCol>,
}

/// PRIMARY first, else the first wholly non-null unique index (by name)
/// without prefix parts and with supported column types.
pub fn choose_key(rows: &[IndexColumn]) -> ChosenKey {
    let mut names: Vec<&str> = rows.iter().map(|r| r.index.as_str()).collect();
    names.dedup();
    names.sort_by_key(|n| (*n != "PRIMARY", n.to_string()));
    names.dedup();
    for name in names {
        let mut cols: Vec<&IndexColumn> = rows.iter().filter(|r| r.index == name).collect();
        cols.sort_by_key(|r| r.seq);
        let usable = !cols.is_empty()
            && mysql_ident(name)
            && cols.iter().all(|c| {
                !c.non_unique
                    && !c.sub_part
                    && !c.nullable
                    && mysql_ident(&c.column)
                    && key_kind(&c.data_type, &c.column_type).is_some()
            });
        if !usable {
            continue;
        }
        let columns: Vec<KeyCol> = cols
            .iter()
            .map(|c| KeyCol {
                name: c.column.clone(),
                kind: key_kind(&c.data_type, &c.column_type).expect("checked"),
                collation: c.collation.clone(),
            })
            .collect();
        let shape = if name != "PRIMARY" {
            KeyShape::UniqueIndex {
                index: name.to_owned(),
                columns: columns.iter().map(|c| c.name.clone()).collect(),
            }
        } else if columns.len() > 1 {
            KeyShape::CompositePrimaryKey {
                columns: columns.iter().map(|c| c.name.clone()).collect(),
            }
        } else if columns[0].kind == KeyKind::Int {
            KeyShape::IntegerPrimaryKey {
                column: columns[0].name.clone(),
            }
        } else {
            KeyShape::StringPrimaryKey {
                column: columns[0].name.clone(),
                collation: columns[0].collation.clone(),
            }
        };
        return ChosenKey {
            shape,
            index: name.to_owned(),
            columns,
        };
    }
    ChosenKey {
        shape: KeyShape::NoKey,
        index: String::new(),
        columns: Vec::new(),
    }
}

/// Whether the lower bound includes its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lower {
    Inclusive,
    Exclusive,
}

fn q(name: &str) -> String {
    format!("`{name}`")
}

/// `WHERE` fragment (possibly empty) and parameters for a key range. The
/// lower bound is `>=` (inclusive) or `>`; the upper bound is always `<`.
pub fn range_predicate<P: Clone>(
    keys: &[KeyCol],
    lo: Option<(&[P], Lower)>,
    hi: Option<&[P]>,
) -> Result<(String, Vec<P>), SourceError> {
    if keys.is_empty() || keys.iter().any(|k| !mysql_ident(&k.name)) {
        return Err(SourceError::config("chunk_key"));
    }
    let mut clauses = Vec::new();
    let mut params = Vec::new();
    let mut side = |values: &[P], lower: Option<Lower>| -> Result<(), SourceError> {
        if values.len() != keys.len() {
            return Err(SourceError::config("chunk_bound_width"));
        }
        let mut alternatives = Vec::new();
        for i in 0..keys.len() {
            let mut terms = Vec::new();
            for j in 0..=i {
                let op = if j < i {
                    "="
                } else {
                    match lower {
                        Some(Lower::Inclusive) if i + 1 == keys.len() => ">=",
                        Some(_) => ">",
                        None => "<",
                    }
                };
                terms.push(format!("{} {op} ?", q(&keys[j].name)));
                params.push(values[j].clone());
            }
            alternatives.push(if terms.len() == 1 {
                terms.remove(0)
            } else {
                format!("({})", terms.join(" AND "))
            });
        }
        clauses.push(format!("({})", alternatives.join(" OR ")));
        Ok(())
    };
    if let Some((values, lower)) = lo {
        side(values, Some(lower))?;
    }
    if let Some(values) = hi {
        side(values, None)?;
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

fn table_sql(schema: &str, table: &str) -> Result<String, SourceError> {
    if !mysql_ident(schema) || !mysql_ident(table) {
        return Err(SourceError::config("table_ident"));
    }
    Ok(format!("{}.{}", q(schema), q(table)))
}

fn force(index: &str) -> Result<String, SourceError> {
    if !mysql_ident(index) {
        return Err(SourceError::config("index_ident"));
    }
    Ok(format!(" FORCE INDEX ({})", q(index)))
}

fn key_list(keys: &[KeyCol]) -> String {
    keys.iter()
        .map(|k| q(&k.name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn hint(timeout_ms: Option<u64>) -> String {
    timeout_ms
        .map(|ms| format!("/*+ MAX_EXECUTION_TIME({ms}) */ "))
        .unwrap_or_default()
}

/// One bounded, key-ordered page of a chunk read.
pub struct PageSql<'a> {
    pub schema: &'a str,
    pub table: &'a str,
    pub index: &'a str,
    pub select_list: &'a str,
    pub keys: &'a [KeyCol],
    pub predicate: &'a str,
    pub limit: u64,
    pub timeout_ms: Option<u64>,
}

pub fn page_sql(p: &PageSql<'_>) -> Result<String, SourceError> {
    Ok(format!(
        "SELECT {}{} FROM {}{}{} ORDER BY {} LIMIT {}",
        hint(p.timeout_ms),
        p.select_list,
        table_sql(p.schema, p.table)?,
        force(p.index)?,
        p.predicate,
        key_list(p.keys),
        p.limit
    ))
}

/// Whole-table read of a table without a usable key.
pub fn single_read_sql(
    schema: &str,
    table: &str,
    select_list: &str,
    limit: u64,
    timeout_ms: Option<u64>,
) -> Result<String, SourceError> {
    Ok(format!(
        "SELECT {}{select_list} FROM {} LIMIT {limit}",
        hint(timeout_ms),
        table_sql(schema, table)?
    ))
}

/// Key of the row `offset` rows after the predicate's first row, or no row.
pub fn boundary_sql(
    schema: &str,
    table: &str,
    index: &str,
    keys: &[KeyCol],
    predicate: &str,
    offset: u64,
    timeout_ms: Option<u64>,
) -> Result<String, SourceError> {
    Ok(format!(
        "SELECT {}{} FROM {}{}{predicate} ORDER BY {} LIMIT {offset}, 1",
        hint(timeout_ms),
        key_list(keys),
        table_sql(schema, table)?,
        force(index)?,
        key_list(keys)
    ))
}

/// Ordered key stream for sampling, one page of `limit` keys.
pub fn key_page_sql(
    schema: &str,
    table: &str,
    index: &str,
    keys: &[KeyCol],
    predicate: &str,
    limit: u64,
    timeout_ms: Option<u64>,
) -> Result<String, SourceError> {
    Ok(format!(
        "SELECT {}{} FROM {}{}{predicate} ORDER BY {} LIMIT {limit}",
        hint(timeout_ms),
        key_list(keys),
        table_sql(schema, table)?,
        force(index)?,
        key_list(keys)
    ))
}

pub fn min_max_sql(
    schema: &str,
    table: &str,
    key: &KeyCol,
    timeout_ms: Option<u64>,
) -> Result<String, SourceError> {
    Ok(format!(
        "SELECT {}MIN({k}), MAX({k}) FROM {}",
        hint(timeout_ms),
        table_sql(schema, table)?,
        k = q(&key.name)
    ))
}

/// Row estimate from the optimizer, not `information_schema.TABLES`.
pub fn explain_rows_sql(schema: &str, table: &str) -> Result<String, SourceError> {
    Ok(format!(
        "EXPLAIN FORMAT=TRADITIONAL SELECT 1 FROM {}",
        table_sql(schema, table)?
    ))
}

/// Average stored bytes over at most 1000 rows, for chunk sizing.
pub fn row_bytes_sql(
    schema: &str,
    table: &str,
    columns: &[String],
    timeout_ms: Option<u64>,
) -> Result<String, SourceError> {
    if columns.is_empty() || columns.iter().any(|c| !mysql_ident(c)) {
        return Err(SourceError::config("column_ident"));
    }
    let len = columns
        .iter()
        .map(|c| format!("COALESCE(OCTET_LENGTH({}), 0)", q(c)))
        .collect::<Vec<_>>()
        .join(" + ");
    Ok(format!(
        "SELECT {}COUNT(*), COALESCE(SUM(b), 0) FROM (SELECT ({len}) AS b FROM {} LIMIT 1000) x",
        hint(timeout_ms),
        table_sql(schema, table)?
    ))
}

/// C5 digest of a key range: `COUNT(*)` and modular sums of both 64-bit halves
/// of `MD5(CONCAT_WS(0x1f, ISNULL flags, values))`. Values are cast to
/// BINARY then HEX so separators in values cannot collide, case-only changes
/// change the digest, and mixed column collations cannot fail. ISNULL flags tell NULL from
/// empty, since CONCAT_WS skips NULL arguments. Sums retain duplicate rows;
/// XOR would cancel equal pairs in a no-key table.
pub fn digest_sql(
    schema: &str,
    table: &str,
    index: Option<&str>,
    columns: &[String],
    predicate: &str,
    timeout_ms: Option<u64>,
) -> Result<String, SourceError> {
    if columns.is_empty() || columns.iter().any(|c| !mysql_ident(c)) {
        return Err(SourceError::config("column_ident"));
    }
    let flags = columns
        .iter()
        .map(|c| format!("ISNULL({})", q(c)))
        .collect::<Vec<_>>()
        .join(", ");
    let values = columns
        .iter()
        .map(|c| format!("HEX(CAST({} AS BINARY))", q(c)))
        .collect::<Vec<_>>()
        .join(", ");
    let md5 = format!("MD5(CONCAT_WS(0x1f, CONCAT({flags}), {values}))");
    let half = |from: u32| {
        format!(
            "CAST(MOD(COALESCE(SUM(CAST(CONV(SUBSTRING({md5}, {from}, 16), 16, 10) AS DECIMAL(20,0))), 0), 18446744073709551616) AS UNSIGNED)"
        )
    };
    Ok(format!(
        "SELECT {}COUNT(*), {}, {} FROM {}{}{predicate}",
        hint(timeout_ms),
        half(1),
        half(17),
        table_sql(schema, table)?,
        match index {
            Some(i) => force(i)?,
            None => String::new(),
        }
    ))
}

/// Read-only statement guard. The connector sends only these shapes, never
/// `LOCK`, `FLUSH`, DDL, DML or locking reads.
pub fn guard(sql: &str) -> Result<&'static str, SourceError> {
    let upper = sql.trim_start().to_ascii_uppercase();
    if upper.contains(';') {
        return Err(SourceError::config("statement_not_read_only"));
    }
    for bad in [
        " FOR UPDATE",
        " LOCK IN SHARE MODE",
        " FOR SHARE",
        " INTO OUTFILE",
        " INTO DUMPFILE",
    ] {
        if upper.contains(bad) {
            return Err(SourceError::config("statement_not_read_only"));
        }
    }
    if upper == "START TRANSACTION WITH CONSISTENT SNAPSHOT" {
        return Ok("START TRANSACTION");
    }
    let verbs: [(&str, &'static str); 11] = [
        ("SELECT ", "SELECT"),
        ("SHOW ", "SHOW"),
        ("EXPLAIN SELECT ", "EXPLAIN"),
        ("EXPLAIN FORMAT=TRADITIONAL SELECT ", "EXPLAIN"),
        ("EXPLAIN FORMAT='BRIEF' SELECT ", "EXPLAIN"),
        ("SET SESSION ", "SET SESSION"),
        (
            "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
            "START TRANSACTION",
        ),
        ("COMMIT", "COMMIT"),
        ("ROLLBACK", "ROLLBACK"),
        (
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            "SET TRANSACTION",
        ),
        ("SET TIME_ZONE", "SET SESSION"),
    ];
    for (prefix, verb) in verbs {
        if upper.starts_with(prefix) {
            if verb == "SET SESSION" && upper.contains("GLOBAL") {
                break;
            }
            return Ok(verb);
        }
    }
    Err(SourceError::config("statement_not_read_only"))
}

/// Deterministic source-side sample; only selected keys cross the wire.
pub fn sample_predicate(keys: &[KeyCol], rate: u64) -> Result<String, SourceError> {
    if keys.is_empty() || keys.iter().any(|k| !mysql_ident(&k.name)) || rate == 0 {
        return Err(SourceError::config("sample_key"));
    }
    let values = keys
        .iter()
        .map(|k| format!("HEX({})", q(&k.name)))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!("MOD(CRC32(CONCAT_WS(0x1f, {values})), {rate}) = 0"))
}

/// Chunk-split choice from the playbook.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Split {
    /// Empty table: one unbounded chunk.
    Empty,
    /// Even integer keys: arithmetic step in key units.
    Step { step: i128 },
    /// Probe each boundary with `LIMIT n, 1`.
    Probe,
    /// More than the threshold of chunks: sample one key in `rate`.
    Sample,
}

/// `(MAX - MIN + 1) / rows`.
pub fn distribution_factor(min: i128, max: i128, rows: u64) -> f64 {
    if rows == 0 {
        return f64::INFINITY;
    }
    (max - min + 1) as f64 / rows as f64
}

pub const EVEN_LOW: f64 = 0.05;
pub const EVEN_HIGH: f64 = 100.0;
/// Step chunks target this share of the row budget. The step comes from the
/// optimizer row estimate, which can be low (InnoDB re-samples statistics,
/// e.g. after a restart); with no headroom every chunk then overflows and the
/// loader exhausts its split attempts. 3/4 absorbs a 25% underestimate.
pub const STEP_FILL: f64 = 0.75;

/// Pick the split. `min_max` is set for a single integer key.
pub fn choose_split(
    rows: u64,
    chunk_rows: u64,
    min_max: Option<(i128, i128)>,
    sample_threshold: u64,
) -> Split {
    if rows == 0 && min_max.is_none() {
        return Split::Empty;
    }
    let chunk_rows = chunk_rows.max(1);
    if rows.div_ceil(chunk_rows) > sample_threshold {
        return Split::Sample;
    }
    if let Some((min, max)) = min_max {
        let factor = distribution_factor(min, max, rows.max(1));
        if (EVEN_LOW..=EVEN_HIGH).contains(&factor) {
            let step = ((chunk_rows as f64) * factor * STEP_FILL).ceil().max(1.0) as i128;
            return Split::Step { step };
        }
    }
    if rows.div_ceil(chunk_rows) > sample_threshold {
        Split::Sample
    } else {
        Split::Probe
    }
}

/// `[lo, hi)` integer chunks for a step split. The first chunk has no lower
/// bound and the last none upper, so keys outside `[min, max]` that appear
/// later are still covered.
pub fn step_bounds(min: i128, max: i128, step: i128) -> Vec<(Option<i128>, Option<i128>)> {
    let step = step.max(1);
    let mut out = Vec::new();
    let mut lo: Option<i128> = None;
    let mut next = min.saturating_add(step);
    while next <= max {
        out.push((lo, Some(next)));
        lo = Some(next);
        let Some(n) = next.checked_add(step) else {
            break;
        };
        next = n;
    }
    out.push((lo, None));
    out
}

/// Boundaries from an ordered sample taken at one key in `rate`: every
/// `chunk_rows / rate` samples (at least one) starts a new chunk. Samples
/// arrive in server order; nothing is compared here.
pub fn sample_boundaries<T: Clone>(samples: &[T], rate: u64, chunk_rows: u64) -> Vec<T> {
    let every = (chunk_rows / rate.max(1)).max(1) as usize;
    samples.iter().skip(every).step_by(every).cloned().collect()
}

/// Rows per chunk within the budget, from the average stored row size.
pub fn chunk_rows_for(budget: &ReadBudget, avg_row_bytes: Option<u64>) -> u64 {
    let cap = budget.max_chunk_rows.clamp(1, 100_000);
    match avg_row_bytes {
        // Keep 2x headroom: Arrow and decode overhead exceed stored bytes.
        Some(avg) if avg > 0 => (budget.max_chunk_bytes / (avg * 2)).clamp(1, cap),
        _ => cap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ic(index: &str, seq: u32, column: &str, data_type: &str) -> IndexColumn {
        IndexColumn {
            index: index.into(),
            seq,
            column: column.into(),
            non_unique: false,
            sub_part: false,
            nullable: false,
            data_type: data_type.into(),
            column_type: data_type.into(),
            collation: (data_type == "varchar").then(|| "utf8mb4_general_ci".into()),
        }
    }

    fn col(name: &str, kind: KeyKind) -> KeyCol {
        KeyCol {
            name: name.into(),
            kind,
            collation: None,
        }
    }

    #[test]
    fn detects_every_key_shape() {
        let int = choose_key(&[ic("PRIMARY", 1, "id", "bigint")]);
        assert_eq!(
            int.shape,
            KeyShape::IntegerPrimaryKey {
                column: "id".into()
            }
        );
        let text = choose_key(&[ic("PRIMARY", 1, "k", "varchar")]);
        assert_eq!(
            text.shape,
            KeyShape::StringPrimaryKey {
                column: "k".into(),
                collation: Some("utf8mb4_general_ci".into())
            }
        );
        let comp = choose_key(&[
            ic("PRIMARY", 2, "seq", "int"),
            ic("PRIMARY", 1, "tenant", "varchar"),
        ]);
        assert_eq!(
            comp.shape,
            KeyShape::CompositePrimaryKey {
                columns: vec!["tenant".into(), "seq".into()]
            }
        );
        let mut nullable = ic("a_uq", 1, "maybe", "int");
        nullable.nullable = true;
        let mut prefix = ic("b_uq", 1, "pre", "varchar");
        prefix.sub_part = true;
        let uniq = choose_key(&[nullable, prefix, ic("c_uq", 1, "code", "varchar")]);
        assert_eq!(
            uniq.shape,
            KeyShape::UniqueIndex {
                index: "c_uq".into(),
                columns: vec!["code".into()]
            }
        );
        assert_eq!(uniq.index, "c_uq");
        assert_eq!(choose_key(&[]).shape, KeyShape::NoKey);
        assert_eq!(
            choose_key(&[ic("PRIMARY", 1, "d", "datetime")]).shape,
            KeyShape::NoKey
        );
        // PRIMARY wins over an earlier-named unique index.
        let both = choose_key(&[
            ic("a_uq", 1, "code", "varchar"),
            ic("PRIMARY", 1, "id", "int"),
        ]);
        assert_eq!(both.index, "PRIMARY");
    }

    #[test]
    fn composite_bounds_are_equality_prefix_disjunctions() {
        let keys = [
            col("a", KeyKind::Text),
            col("b", KeyKind::Int),
            col("c", KeyKind::Int),
        ];
        let lo = [1, 2, 3];
        let hi = [7, 8, 9];
        let (sql, params) =
            range_predicate(&keys, Some((&lo[..], Lower::Inclusive)), Some(&hi[..])).unwrap();
        assert_eq!(
            sql,
            " WHERE (`a` > ? OR (`a` = ? AND `b` > ?) OR (`a` = ? AND `b` = ? AND `c` >= ?)) \
             AND (`a` < ? OR (`a` = ? AND `b` < ?) OR (`a` = ? AND `b` = ? AND `c` < ?))"
        );
        assert_eq!(params, [1, 1, 2, 1, 2, 3, 7, 7, 8, 7, 8, 9]);
        let (sql, _) =
            range_predicate(&keys[..2], Some((&lo[..2], Lower::Exclusive)), None).unwrap();
        assert_eq!(sql, " WHERE (`a` > ? OR (`a` = ? AND `b` > ?))");
        for forbidden in ["COLLATE", "BINARY", "(`a`, `b`)"] {
            assert!(!sql.contains(forbidden));
        }
    }

    #[test]
    fn single_key_bounds_and_errors() {
        let keys = [col("k", KeyKind::Text)];
        let (sql, p) = range_predicate(
            &keys,
            Some((&["a"][..], Lower::Inclusive)),
            Some(&["B"][..]),
        )
        .unwrap();
        assert_eq!(sql, " WHERE (`k` >= ?) AND (`k` < ?)");
        assert_eq!(p, ["a", "B"]);
        assert_eq!(range_predicate::<u8>(&keys, None, None).unwrap().0, "");
        assert!(range_predicate(&keys, None, Some(&[1, 2][..])).is_err());
        assert!(range_predicate::<u8>(&[], None, None).is_err());
        assert!(range_predicate::<u8>(&[col("bad`name", KeyKind::Int)], None, None).is_err());
    }

    #[test]
    fn reads_force_the_key_index_and_bound_the_server_time() {
        let keys = [col("tenant", KeyKind::Text), col("seq", KeyKind::Int)];
        let sql = page_sql(&PageSql {
            schema: "s",
            table: "t",
            index: "PRIMARY",
            select_list: "`tenant`, `seq`, `v`",
            keys: &keys,
            predicate: " WHERE (`tenant` >= ?)",
            limit: 500,
            timeout_ms: Some(30_000),
        })
        .unwrap();
        assert_eq!(
            sql,
            "SELECT /*+ MAX_EXECUTION_TIME(30000) */ `tenant`, `seq`, `v` FROM `s`.`t` FORCE INDEX (`PRIMARY`) WHERE (`tenant` >= ?) ORDER BY `tenant`, `seq` LIMIT 500"
        );
        let b = boundary_sql("s", "t", "PRIMARY", &keys, "", 1000, None).unwrap();
        assert_eq!(
            b,
            "SELECT `tenant`, `seq` FROM `s`.`t` FORCE INDEX (`PRIMARY`) ORDER BY `tenant`, `seq` LIMIT 1000, 1"
        );
        for sql in [&sql, &b] {
            assert_eq!(guard(sql).unwrap(), "SELECT");
        }
        let bad = PageSql {
            schema: "s",
            table: "t",
            index: "x;DROP",
            select_list: "1",
            keys: &keys,
            predicate: "",
            limit: 1,
            timeout_ms: None,
        };
        assert!(page_sql(&bad).is_err());
    }

    #[test]
    fn digest_sql_flags_nulls_and_casts_to_binary() {
        let sql = digest_sql(
            "s",
            "t",
            Some("PRIMARY"),
            &["id".into(), "v".into()],
            " WHERE (`id` >= ?)",
            Some(5000),
        )
        .unwrap();
        assert!(sql.starts_with("SELECT /*+ MAX_EXECUTION_TIME(5000) */ COUNT(*), CAST(MOD(COALESCE(SUM(CAST(CONV(SUBSTRING(MD5(CONCAT_WS(0x1f, CONCAT(ISNULL(`id`), ISNULL(`v`)), HEX(CAST(`id` AS BINARY)), HEX(CAST(`v` AS BINARY)))), 1, 16), 16, 10) AS DECIMAL(20,0)))"));
        assert!(sql.contains("18446744073709551616"));
        assert!(!sql.contains("BIT_XOR"));
        assert!(sql.contains(", 17, 16)"));
        assert!(sql.ends_with("FROM `s`.`t` FORCE INDEX (`PRIMARY`) WHERE (`id` >= ?)"));
        assert_eq!(guard(&sql).unwrap(), "SELECT");
    }

    #[test]
    fn guard_allows_only_read_only_shapes() {
        for ok in [
            "SELECT 1",
            "SHOW GLOBAL STATUS LIKE 'Threads_running'",
            "EXPLAIN SELECT 1 FROM `s`.`t`",
            "EXPLAIN FORMAT=TRADITIONAL SELECT 1 FROM `s`.`t`",
            "SET SESSION net_read_timeout = 600",
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
            "COMMIT",
        ] {
            assert!(guard(ok).is_ok(), "{ok}");
        }
        for bad in [
            "LOCK TABLES t READ",
            "FLUSH TABLES WITH READ LOCK",
            "CREATE TABLE x (a int)",
            "INSERT INTO t VALUES (1)",
            "SELECT * FROM t FOR UPDATE",
            "SELECT * FROM t LOCK IN SHARE MODE",
            "SET GLOBAL read_only = 1",
            "SET SESSION GLOBAL x = 1",
            "START TRANSACTION",
            "KILL 5",
            "COMMIT; DELETE FROM t",
            "SELECT 1; DROP TABLE t",
        ] {
            assert!(guard(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn distribution_factor_selects_step_probe_or_sample() {
        // Dense ids: step is chunk rows less the estimate headroom.
        assert_eq!(
            choose_split(2_000_000, 100_000, Some((1, 2_000_000)), 1000),
            Split::Step { step: 75_000 }
        );
        // Sparse but within 100x: step scales with the factor.
        assert_eq!(
            choose_split(1000, 100, Some((1, 50_000)), 1000),
            Split::Step { step: 3750 }
        );
        // A 20% low estimate (16,000 for dense ids 1..=20,000) still fits 500 rows.
        assert_eq!(
            choose_split(16_000, 500, Some((1, 20_000)), 1000),
            Split::Step { step: 469 }
        );
        // Factor above 100: probe boundaries.
        assert_eq!(
            choose_split(1000, 100, Some((1, 1_000_000)), 1000),
            Split::Probe
        );
        // Factor below 0.05 (stale estimate): probe.
        assert_eq!(
            choose_split(1_000_000, 100, Some((1, 10)), 100_000),
            Split::Probe
        );
        // Many chunks: sample.
        assert_eq!(
            choose_split(200_000_000, 100_000, None, 1000),
            Split::Sample
        );
        assert_eq!(
            choose_split(200_000_000, 100_000, Some((1, 200_000_000)), 1000),
            Split::Sample
        );
        assert_eq!(choose_split(2_000_000, 100_000, None, 1000), Split::Probe);
        assert_eq!(choose_split(0, 100, None, 1000), Split::Empty);
        assert!(distribution_factor(1, 10, 0).is_infinite());
    }

    #[test]
    fn step_bounds_cover_the_whole_key_space() {
        assert_eq!(
            step_bounds(1, 10, 4),
            vec![(None, Some(5)), (Some(5), Some(9)), (Some(9), None)]
        );
        assert_eq!(step_bounds(5, 5, 100), vec![(None, None)]);
        let b = step_bounds(1, 2_000_000, 100_000);
        assert_eq!(b.len(), 20);
        assert!(b.windows(2).all(|w| w[0].1 == w[1].0));
    }

    #[test]
    fn sampled_boundaries_keep_server_order() {
        assert_eq!(
            sample_predicate(&[col("k", KeyKind::Text)], 1000).unwrap(),
            "MOD(CRC32(CONCAT_WS(0x1f, HEX(`k`))), 1000) = 0"
        );
        let samples: Vec<u32> = (0..25).collect();
        assert_eq!(sample_boundaries(&samples, 1000, 5000), vec![5, 10, 15, 20]);
        assert_eq!(
            sample_boundaries(&samples, 1000, 10),
            (1..25).collect::<Vec<_>>()
        );
        assert!(sample_boundaries::<u32>(&[], 1000, 5000).is_empty());
    }

    #[test]
    fn chunk_rows_respect_bytes_and_hard_caps() {
        let b = ReadBudget::default();
        assert_eq!(chunk_rows_for(&b, None), 100_000);
        assert_eq!(chunk_rows_for(&b, Some(100)), 100_000);
        assert_eq!(chunk_rows_for(&b, Some(4096)), 4096);
        let big = ReadBudget {
            max_chunk_rows: 10_000_000,
            ..ReadBudget::default()
        };
        assert_eq!(chunk_rows_for(&big, None), 100_000);
    }
}
