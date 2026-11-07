//! Pure SQL builders for the Oracle engine: keyset predicates, pages,
//! boundary probes, ROWID ranges and range digests.
//!
//! Key bounds are always bind values compared by Oracle under the key
//! column's own collation. The client never compares strings and never wraps
//! a key in `NLSSORT`, which would make the key index unusable.

use crate::SourceError;
use crate::engine::KeyValue;

use super::rules::Dialect;

/// How a column is projected to canonical text and bound as a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColKind {
    Number,
    BinaryFloat,
    /// VARCHAR2 / CHAR: Oracle stores '' as NULL.
    Char,
    /// NVARCHAR2 / NCHAR: bound through AL32UTF8 bytes, never via the
    /// database character set.
    NChar,
    Raw,
    Clob,
    NClob,
    Blob,
    Date,
    Timestamp,
    TimestampTz,
}

impl ColKind {
    pub fn from_oracle(data_type: &str) -> Option<Self> {
        Some(match data_type {
            "NUMBER" | "FLOAT" | "INTEGER" => Self::Number,
            "BINARY_FLOAT" | "BINARY_DOUBLE" => Self::BinaryFloat,
            "VARCHAR2" | "CHAR" => Self::Char,
            "NVARCHAR2" | "NCHAR" => Self::NChar,
            "RAW" => Self::Raw,
            "CLOB" => Self::Clob,
            "NCLOB" => Self::NClob,
            "BLOB" => Self::Blob,
            "DATE" => Self::Date,
            t if t.starts_with("TIMESTAMP(") && t.ends_with("TIME ZONE") => Self::TimestampTz,
            t if t.starts_with("TIMESTAMP(") && t.ends_with(')') => Self::Timestamp,
            _ => return None,
        })
    }

    /// Canonical label recorded in Arrow field metadata.
    pub fn canonical(self) -> &'static str {
        match self {
            Self::Number => "decimal",
            Self::BinaryFloat => "ieee754_be",
            Self::Char | Self::NChar | Self::Clob | Self::NClob => "text",
            Self::Raw | Self::Blob => "hex",
            Self::Date | Self::Timestamp | Self::TimestampTz => "iso8601",
        }
    }

    pub fn is_text(self) -> bool {
        matches!(self, Self::Char | Self::NChar)
    }

    /// Keys whose order Oracle defines and the client can bind losslessly.
    pub fn keyable(self) -> bool {
        !matches!(self, Self::TimestampTz | Self::BinaryFloat) && !self.is_lob()
    }

    pub fn is_lob(self) -> bool {
        matches!(self, Self::Clob | Self::NClob | Self::Blob)
    }

    /// Upper bound of the canonical text length in bytes.
    pub fn max_text(self, data_length: u32) -> u32 {
        match self {
            Self::Number | Self::BinaryFloat => 66,
            Self::Char => data_length,
            Self::NChar => data_length.saturating_mul(4),
            Self::Raw => data_length.saturating_mul(2),
            Self::Date => 21,
            Self::Timestamp => 31,
            Self::TimestampTz => 38,
            Self::Clob | Self::NClob | Self::Blob => u32::MAX,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub kind: ColKind,
    pub nullable: bool,
    pub data_length: u32,
    pub precision: Option<u8>,
    pub scale: Option<i8>,
    /// `ALL_TAB_COLS.COLLATION` on 12.2+, `None` before.
    pub collation: Option<String>,
}

/// Quote an identifier from the catalog. Quotes and control characters are
/// rejected so no catalog text can close the identifier.
pub fn ident(name: &str) -> Result<String, SourceError> {
    if name.is_empty()
        || name.len() > 128
        || name.contains('"')
        || name.chars().any(char::is_control)
    {
        return Err(SourceError::identifier("oracle_identifier"));
    }
    Ok(format!("\"{name}\""))
}

/// Canonical text expression of one column, independent of session NLS.
pub fn projection(c: &Column) -> Result<String, SourceError> {
    let n = ident(&c.name)?;
    Ok(match c.kind {
        ColKind::Number => format!("TO_CHAR({n},'TM9','NLS_NUMERIC_CHARACTERS=''.,''')"),
        ColKind::BinaryFloat => {
            let function = if c.data_type == "BINARY_FLOAT" {
                "CAST_FROM_BINARY_FLOAT"
            } else {
                "CAST_FROM_BINARY_DOUBLE"
            };
            format!("RAWTOHEX(UTL_RAW.{function}({n},1))")
        }
        ColKind::Char | ColKind::NChar | ColKind::Clob | ColKind::NClob | ColKind::Blob => n,
        ColKind::Raw => format!("RAWTOHEX({n})"),
        ColKind::Date => format!("TO_CHAR({n},'SYYYY-MM-DD\"T\"HH24:MI:SS')"),
        ColKind::Timestamp => format!("TO_CHAR({n},'SYYYY-MM-DD\"T\"HH24:MI:SS.FF9')"),
        ColKind::TimestampTz => {
            format!("TO_CHAR({n} AT TIME ZONE 'UTC','SYYYY-MM-DD\"T\"HH24:MI:SS.FF9 TZH:TZM')")
        }
    })
}

/// Digest text of one column: a NULL flag plus canonical text. National
/// text is hashed as AL32UTF8 bytes so the row text stays VARCHAR2.
fn digest_cell(c: &Column) -> Result<String, SourceError> {
    if c.kind.is_lob() {
        return Err(SourceError::catalog("lob_source_digest_requires_streaming"));
    }
    let n = ident(&c.name)?;
    let text = match c.kind {
        ColKind::NChar => format!("RAWTOHEX(UTL_I18N.STRING_TO_RAW({n},'AL32UTF8'))"),
        _ => projection(c)?,
    };
    // Length framing distinguishes embedded delimiters from field boundaries.
    Ok(format!(
        "NVL2({n},'v'||TO_CHAR(LENGTHB({text}),'FM9999999990')||':'||{text},'n')"
    ))
}

/// One bind value, typed for the driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bind {
    /// Canonical decimal text, bound as an Oracle NUMBER.
    Number(String),
    Text(String),
}

/// SQL text plus its positional binds `:b1..:bn`, in order of appearance.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sql {
    pub text: String,
    pub binds: Vec<Bind>,
}

impl Sql {
    fn bind(&mut self, b: Bind) -> String {
        self.binds.push(b);
        format!(":b{}", self.binds.len())
    }
}

/// Bind expression for one key value of column `c`.
fn key_expr(sql: &mut Sql, c: &Column, v: &KeyValue) -> Result<String, SourceError> {
    let bad = || SourceError::catalog("key_value_shape");
    Ok(match (c.kind, v) {
        (ColKind::Number, KeyValue::Int(i)) => sql.bind(Bind::Number(i.to_string())),
        (ColKind::Number, KeyValue::Decimal(d)) => {
            sql.bind(Bind::Number(super::chunk::decimal(d).map_err(|_| bad())?))
        }
        (ColKind::Char, KeyValue::Text(t)) => sql.bind(Bind::Text(t.clone())),
        (ColKind::NChar, KeyValue::Text(t)) => {
            let b = sql.bind(Bind::Text(super::chunk::hex(t.as_bytes())));
            format!("UTL_I18N.RAW_TO_NCHAR(HEXTORAW({b}),'AL32UTF8')")
        }
        (ColKind::Raw, KeyValue::Bytes(h)) => {
            if h.len() % 2 != 0 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(bad());
            }
            format!("HEXTORAW({})", sql.bind(Bind::Text(h.clone())))
        }
        (ColKind::Date, KeyValue::Text(t)) => format!(
            "TO_DATE({},'SYYYY-MM-DD\"T\"HH24:MI:SS')",
            sql.bind(Bind::Text(t.clone()))
        ),
        (ColKind::Timestamp, KeyValue::Text(t)) => format!(
            "TO_TIMESTAMP({},'SYYYY-MM-DD\"T\"HH24:MI:SS.FF9')",
            sql.bind(Bind::Text(t.clone()))
        ),
        _ => return Err(bad()),
    })
}

/// Key value from canonical text as projected by [`projection`].
pub fn key_value(kind: ColKind, text: &str) -> Result<KeyValue, SourceError> {
    Ok(match kind {
        ColKind::Number => {
            let d =
                super::chunk::decimal(text).map_err(|_| SourceError::protocol("number_text"))?;
            match d.parse::<i128>() {
                Ok(i) if !d.contains('.') => KeyValue::Int(i),
                _ => KeyValue::Decimal(d),
            }
        }
        ColKind::Raw => KeyValue::Bytes(text.to_ascii_lowercase()),
        _ => KeyValue::Text(text.to_owned()),
    })
}

/// A bound of a key range. `inclusive` applies to the full tuple.
#[derive(Clone, Copy, Debug)]
pub struct Bound<'a> {
    pub values: &'a [KeyValue],
    pub inclusive: bool,
}

/// Row-value comparison rewritten for Oracle:
/// `(a,b) > (x,y)` becomes `a >= x AND (a > x OR (a = x AND b > y))`.
/// The leading `a >= x` lets the optimizer use an index range scan; the
/// rewrite does not rely on row-value comparison support in any version.
fn tuple_cmp(
    sql: &mut Sql,
    keys: &[&Column],
    b: Bound<'_>,
    greater: bool,
) -> Result<String, SourceError> {
    if b.values.len() != keys.len() || keys.is_empty() {
        return Err(SourceError::catalog("key_arity"));
    }
    if b.values.iter().any(|v| matches!(v, KeyValue::Null)) {
        return Err(SourceError::catalog("null_key_bound"));
    }
    let (strict, loose) = if greater { (">", ">=") } else { ("<", "<=") };
    let lead_col = ident(&keys[0].name)?;
    let lead = key_expr(sql, keys[0], &b.values[0])?;
    if keys.len() == 1 {
        let op = if b.inclusive { loose } else { strict };
        return Ok(format!("{lead_col} {op} {lead}"));
    }
    let mut alternatives = Vec::new();
    for end in 0..keys.len() {
        let mut terms = Vec::new();
        for (j, col) in keys.iter().enumerate().take(end + 1) {
            let op = if j < end {
                "="
            } else if end + 1 == keys.len() && b.inclusive {
                loose
            } else {
                strict
            };
            let e = key_expr(sql, col, &b.values[j])?;
            terms.push(format!("{} {op} {e}", ident(&col.name)?));
        }
        alternatives.push(format!("({})", terms.join(" AND ")));
    }
    Ok(format!(
        "{lead_col} {loose} {lead} AND ({})",
        alternatives.join(" OR ")
    ))
}

/// `WHERE` clause body for `lower .. upper`; empty when unbounded.
pub fn range_predicate(
    sql: &mut Sql,
    keys: &[&Column],
    lower: Option<Bound<'_>>,
    upper: Option<Bound<'_>>,
) -> Result<String, SourceError> {
    let mut parts = Vec::new();
    if let Some(b) = lower {
        parts.push(format!("({})", tuple_cmp(sql, keys, b, true)?));
    }
    if let Some(b) = upper {
        parts.push(format!("({})", tuple_cmp(sql, keys, b, false)?));
    }
    Ok(parts.join(" AND "))
}

pub fn table_name(owner: &str, name: &str) -> Result<String, SourceError> {
    Ok(format!("{}.{}", ident(owner)?, ident(name)?))
}

fn order_by(keys: &[&Column]) -> Result<String, SourceError> {
    Ok(keys
        .iter()
        .map(|c| ident(&c.name))
        .collect::<Result<Vec<_>, _>>()?
        .join(","))
}

/// Physical ROWID range predicate `[lo, hi)`.
pub fn rowid_predicate(sql: &mut Sql, lo: Option<&str>, hi: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(lo) = lo {
        let b = sql.bind(Bind::Text(lo.to_owned()));
        parts.push(format!("ROWID >= CHARTOROWID({b})"));
    }
    if let Some(hi) = hi {
        let b = sql.bind(Bind::Text(hi.to_owned()));
        parts.push(format!("ROWID < CHARTOROWID({b})"));
    }
    parts.join(" AND ")
}

/// Where the page predicate comes from.
pub enum PageRange<'a> {
    Keyset {
        keys: &'a [&'a Column],
        lower: Option<Bound<'a>>,
        upper: Option<Bound<'a>>,
    },
    Rowid {
        lo: Option<&'a str>,
        hi: Option<&'a str>,
    },
}

/// One bounded page of a chunk at one SCN, ordered by key when keyed.
/// `limit` rows at most; the caller asks for one more than it accepts.
pub fn page_sql(
    d: &Dialect,
    owner: &str,
    table: &str,
    columns: &[Column],
    range: PageRange<'_>,
    scn: u64,
    limit: u64,
) -> Result<Sql, SourceError> {
    let mut sql = Sql::default();
    let projected = columns
        .iter()
        .map(projection)
        .collect::<Result<Vec<_>, _>>()?
        .join(",");
    let from = table_name(owner, table)?;
    // Binds follow text order: the AS OF SCN bind precedes the predicate.
    let scn_b = sql.bind(Bind::Number(scn.to_string()));
    let (pred, order) = match range {
        PageRange::Keyset { keys, lower, upper } => (
            range_predicate(&mut sql, keys, lower, upper)?,
            Some(order_by(keys)?),
        ),
        PageRange::Rowid { lo, hi } => (rowid_predicate(&mut sql, lo, hi), None),
    };
    let where_ = if pred.is_empty() {
        String::new()
    } else {
        format!(" WHERE {pred}")
    };
    let order_ = order.map(|o| format!(" ORDER BY {o}")).unwrap_or_default();
    let inner = format!("SELECT {projected} FROM {from} AS OF SCN {scn_b}{where_}{order_}");
    // Row-limit binds hit ORA-03089 with the current thin driver/server pair.
    // This is a typed internal budget, never caller SQL or a source value.
    let lim = limit.to_string();
    sql.text = if d.fetch_first {
        format!("{inner} FETCH FIRST {lim} ROWS ONLY")
    } else {
        format!("SELECT * FROM ({inner}) WHERE ROWNUM <= {lim}")
    };
    Ok(sql)
}

/// Key of the row `step` positions at or after `lower` in Oracle's key
/// order, or nothing when the range is shorter. No snapshot: any existing
/// key is a valid boundary.
pub fn boundary_probe_sql(
    d: &Dialect,
    owner: &str,
    table: &str,
    keys: &[&Column],
    lower: Option<Bound<'_>>,
    upper: Option<Bound<'_>>,
    step: u64,
) -> Result<Sql, SourceError> {
    let mut sql = Sql::default();
    let pred = range_predicate(&mut sql, keys, lower, upper)?;
    let where_ = if pred.is_empty() {
        String::new()
    } else {
        format!(" WHERE {pred}")
    };
    let cols = keys
        .iter()
        .map(|c| projection(c))
        .collect::<Result<Vec<_>, _>>()?;
    let names = order_by(keys)?;
    let from = table_name(owner, table)?;
    if d.fetch_first {
        let off = step.to_string();
        sql.text = format!(
            "SELECT {} FROM {from}{where_} ORDER BY {names} OFFSET {off} ROWS FETCH NEXT 1 ROWS ONLY",
            cols.join(",")
        );
    } else {
        let aliases: Vec<String> = (0..keys.len()).map(|i| format!("K{i}")).collect();
        let projected: Vec<String> = cols
            .iter()
            .zip(&aliases)
            .map(|(c, a)| format!("{c} {a}"))
            .collect();
        let stop = step.saturating_add(1).to_string();
        let at = stop.clone();
        sql.text = format!(
            "SELECT {a} FROM (SELECT {a}, ROWNUM OO_RN FROM (SELECT {p} FROM {from}{where_} ORDER BY {names}) WHERE ROWNUM <= {stop}) WHERE OO_RN = {at}",
            a = aliases.join(","),
            p = projected.join(",")
        );
    }
    Ok(sql)
}

/// Count and four order-independent hash lanes over `[lower, upper)`.
/// Each row hashes NULL flags plus canonical text; lanes are summed so row
/// order does not matter. Wide rows are hashed per segment first so no
/// intermediate VARCHAR2 exceeds 4000 bytes.
pub fn digest_sql(
    d: &Dialect,
    owner: &str,
    table: &str,
    columns: &[Column],
    range: PageRange<'_>,
    scn: Option<u64>,
) -> Result<Sql, SourceError> {
    let mut sql = Sql::default();
    let snapshot = scn
        .map(|scn| format!(" AS OF SCN {}", sql.bind(Bind::Number(scn.to_string()))))
        .unwrap_or_default();
    let pred = match range {
        PageRange::Keyset { keys, lower, upper } => range_predicate(&mut sql, keys, lower, upper)?,
        PageRange::Rowid { lo, hi } => rowid_predicate(&mut sql, lo, hi),
    };
    let where_ = if pred.is_empty() {
        String::new()
    } else {
        format!(" WHERE {pred}")
    };
    let row_text = digest_row_text(d, columns)?;
    let from = format!("{}{snapshot}", table_name(owner, table)?);
    sql.text = if d.standard_hash {
        let lanes: Vec<String> = (0..4)
            .map(|i| {
                format!(
                    "SUM(TO_NUMBER(SUBSTR(H,{},15),'XXXXXXXXXXXXXXX'))",
                    1 + 15 * i
                )
            })
            .collect();
        format!(
            "SELECT TO_CHAR(COUNT(*)),{} FROM (SELECT RAWTOHEX(STANDARD_HASH({row_text},'SHA256')) H FROM {from}{where_})",
            lanes
                .iter()
                .map(|l| format!("TO_CHAR({l})"))
                .collect::<Vec<_>>()
                .join(",")
        )
    } else {
        let lanes: Vec<String> = (1..=4)
            .map(|seed| format!("TO_CHAR(SUM(ORA_HASH(R,4294967295,{seed})))"))
            .collect();
        format!(
            "SELECT TO_CHAR(COUNT(*)),{} FROM (SELECT {row_text} R FROM {from}{where_})",
            lanes.join(",")
        )
    };
    Ok(sql)
}

/// Byte budget for one intermediate VARCHAR2 in SQL.
const SEGMENT_BYTES: u32 = 3800;

fn digest_row_text(d: &Dialect, columns: &[Column]) -> Result<String, SourceError> {
    let cells: Vec<(String, u32)> = columns
        .iter()
        .map(|c| {
            let bytes = c.kind.max_text(c.data_length);
            let bytes = if c.kind == ColKind::NChar {
                bytes.saturating_mul(2)
            } else {
                bytes
            };
            Ok((digest_cell(c)?, bytes.saturating_add(13)))
        })
        .collect::<Result<_, SourceError>>()?;
    let join = |cells: &[(String, u32)]| {
        cells
            .iter()
            .map(|(e, _)| e.as_str())
            .collect::<Vec<_>>()
            .join("||CHR(31)||")
    };
    let total: u32 = cells.iter().map(|(_, n)| n).sum();
    if total <= SEGMENT_BYTES {
        return Ok(join(&cells));
    }
    let mut segments = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (i, (_, n)) in cells.iter().enumerate() {
        if *n > SEGMENT_BYTES {
            return Err(SourceError::budget("digest_column_too_wide"));
        }
        if used + n > SEGMENT_BYTES {
            segments.push(&cells[start..i]);
            start = i;
            used = 0;
        }
        used += n;
    }
    segments.push(&cells[start..]);
    let hashed: Vec<String> = segments
        .iter()
        .map(|s| {
            let text = join(s);
            if d.standard_hash {
                format!("RAWTOHEX(STANDARD_HASH({text},'SHA256'))")
            } else {
                format!(
                    "TO_CHAR(ORA_HASH({text},4294967295,1))||'.'||TO_CHAR(ORA_HASH({text},4294967295,2))"
                )
            }
        })
        .collect();
    Ok(hashed.join("||'|'||"))
}

/// One extent of a table segment as `DBA_EXTENTS` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub data_object_id: u64,
    pub relative_fno: u32,
    pub block_id: u64,
    pub blocks: u64,
}

/// `(relative_fno, block)` arguments for `DBMS_ROWID.ROWID_CREATE`. Bigfile
/// tablespaces report `RELATIVE_FNO = 1024` and a 32-bit block number that
/// spans the file and block fields of the ROWID.
pub fn rowid_parts(e: &Extent) -> (u64, u64) {
    if e.relative_fno == 1024 {
        (e.block_id >> 22, e.block_id & 0x3F_FFFF)
    } else {
        (u64::from(e.relative_fno), e.block_id)
    }
}

/// Group sorted extents into chunks of about `blocks_per_chunk` blocks.
/// Returns the index of the first extent of each chunk. Chunk `i` covers
/// `[start(i), start(i+1))`; the first chunk starts unbounded and the last
/// ends unbounded, so no row of the snapshot can fall outside every chunk.
pub fn group_extents(extents: &[Extent], blocks_per_chunk: u64) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut acc = 0u64;
    for (i, e) in extents.iter().enumerate() {
        if i == 0 || acc >= blocks_per_chunk.max(1) {
            starts.push(i);
            acc = 0;
        }
        acc += e.blocks;
    }
    starts
}

/// Large extents are allocation units, not a safe upper bound on chunk size.
pub fn split_extents(extents: &[Extent], blocks_per_chunk: u64) -> Vec<Extent> {
    let mut out = Vec::new();
    let limit = blocks_per_chunk.max(1);
    for e in extents {
        let mut offset = 0;
        while offset < e.blocks {
            let blocks = (e.blocks - offset).min(limit);
            out.push(Extent {
                block_id: e.block_id + offset,
                blocks,
                ..*e
            });
            offset += blocks;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{EngineFamily, EngineVersion};

    #[test]
    fn local_timezone_is_projected_as_an_explicit_utc_instant() {
        let c = col("TS", "TIMESTAMP(6) WITH LOCAL TIME ZONE");
        assert_eq!(c.kind, ColKind::TimestampTz);
        assert!(projection(&c).unwrap().contains("AT TIME ZONE 'UTC'"));
        assert!(!c.kind.keyable());
        for kind in ["BINARY_FLOAT", "BINARY_DOUBLE"] {
            assert_eq!(ColKind::from_oracle(kind), Some(ColKind::BinaryFloat));
        }
    }

    fn col(name: &str, data_type: &str) -> Column {
        Column {
            name: name.into(),
            data_type: data_type.into(),
            kind: ColKind::from_oracle(data_type).unwrap(),
            nullable: false,
            data_length: 100,
            precision: None,
            scale: None,
            collation: None,
        }
    }

    fn dialect(raw: &str) -> Dialect {
        Dialect::for_version(&EngineVersion::parse(EngineFamily::Oracle, raw).unwrap())
    }

    #[test]
    fn composite_bound_is_rewritten_with_leading_index_term() {
        let a = col("A", "NUMBER");
        let b = col("B", "VARCHAR2");
        let keys = [&a, &b];
        let lo = [KeyValue::Int(5), KeyValue::Text("x' OR 1=1".into())];
        let hi = [KeyValue::Int(9), KeyValue::Text("m".into())];
        let mut sql = Sql::default();
        let p = range_predicate(
            &mut sql,
            &keys,
            Some(Bound {
                values: &lo,
                inclusive: true,
            }),
            Some(Bound {
                values: &hi,
                inclusive: false,
            }),
        )
        .unwrap();
        assert_eq!(
            p,
            "(\"A\" >= :b1 AND ((\"A\" > :b2) OR (\"A\" = :b3 AND \"B\" >= :b4))) AND (\"A\" <= :b5 AND ((\"A\" < :b6) OR (\"A\" = :b7 AND \"B\" < :b8)))"
        );
        assert!(!p.contains("x'"));
        assert!(!p.to_ascii_uppercase().contains("NLSSORT"));
        assert_eq!(sql.binds.len(), 8);
        assert_eq!(sql.binds[3], Bind::Text("x' OR 1=1".into()));
    }

    #[test]
    fn single_key_bounds_and_types() {
        let s = col("S", "NVARCHAR2");
        let mut sql = Sql::default();
        let v = [KeyValue::Text("é".into())];
        let p = range_predicate(
            &mut sql,
            &[&s],
            Some(Bound {
                values: &v,
                inclusive: false,
            }),
            None,
        )
        .unwrap();
        assert_eq!(
            p,
            "(\"S\" > UTL_I18N.RAW_TO_NCHAR(HEXTORAW(:b1),'AL32UTF8'))"
        );
        assert_eq!(sql.binds, [Bind::Text("c3a9".into())]);
        let mut sql = Sql::default();
        assert!(
            range_predicate(
                &mut sql,
                &[&s],
                Some(Bound {
                    values: &[KeyValue::Null],
                    inclusive: true
                }),
                None
            )
            .is_err()
        );
        for name in ["CLOB", "NCLOB", "BLOB"] {
            let kind = ColKind::from_oracle(name).unwrap();
            assert!(kind.is_lob());
            assert!(!kind.keyable());
            assert!(!kind.is_text(), "empty LOBs are not Oracle empty strings");
            assert_eq!(projection(&col("L", name)).unwrap(), "\"L\"");
            assert!(digest_cell(&col("L", name)).is_err());
        }
        assert_eq!(
            ColKind::from_oracle("TIMESTAMP(6) WITH LOCAL TIME ZONE"),
            Some(ColKind::TimestampTz)
        );
        assert!(
            !ColKind::from_oracle("TIMESTAMP(6) WITH TIME ZONE")
                .unwrap()
                .keyable()
        );
    }

    #[test]
    fn page_sql_dispatches_fetch_first_or_rownum() {
        let a = col("A", "NUMBER");
        let cols = vec![a.clone(), col("B", "VARCHAR2")];
        let keys = [&cols[0]];
        let lo = [KeyValue::Int(10)];
        let range = || PageRange::Keyset {
            keys: &keys,
            lower: Some(Bound {
                values: &lo,
                inclusive: true,
            }),
            upper: None,
        };
        let new = page_sql(&dialect("23.26.3.0.0"), "APP", "T", &cols, range(), 77, 101).unwrap();
        assert_eq!(
            new.text,
            "SELECT TO_CHAR(\"A\",'TM9','NLS_NUMERIC_CHARACTERS=''.,'''),\"B\" FROM \"APP\".\"T\" AS OF SCN :b1 WHERE (\"A\" >= :b2) ORDER BY \"A\" FETCH FIRST 101 ROWS ONLY"
        );
        assert_eq!(
            new.binds,
            [Bind::Number("77".into()), Bind::Number("10".into())]
        );
        let old = page_sql(&dialect("11.2.0.2.0"), "APP", "T", &cols, range(), 77, 101).unwrap();
        assert!(old.text.starts_with("SELECT * FROM (SELECT "));
        assert!(old.text.ends_with(") WHERE ROWNUM <= 101"));
        assert!(!old.text.contains("FETCH"));
        let rid = page_sql(
            &dialect("21.3.0.0.0"),
            "APP",
            "H",
            &cols,
            PageRange::Rowid {
                lo: Some("AAAR1"),
                hi: None,
            },
            5,
            10,
        )
        .unwrap();
        assert!(
            rid.text
                .contains("AS OF SCN :b1 WHERE ROWID >= CHARTOROWID(:b2) FETCH")
        );
        assert!(!rid.text.contains("ORDER BY"));
    }

    #[test]
    fn boundary_probe_uses_offset_or_rownum_without_snapshot() {
        let a = col("A", "VARCHAR2");
        let keys = [&a];
        let new =
            boundary_probe_sql(&dialect("19.3.0.0.0"), "APP", "T", &keys, None, None, 500).unwrap();
        assert_eq!(
            new.text,
            "SELECT \"A\" FROM \"APP\".\"T\" ORDER BY \"A\" OFFSET 500 ROWS FETCH NEXT 1 ROWS ONLY"
        );
        assert!(!new.text.contains("AS OF"));
        let old =
            boundary_probe_sql(&dialect("11.2.0.2.0"), "APP", "T", &keys, None, None, 500).unwrap();
        assert!(old.text.contains("WHERE ROWNUM <= 501) WHERE OO_RN = 501"));
        assert!(old.binds.is_empty());
    }

    #[test]
    fn digest_dispatches_hash_and_segments_wide_rows() {
        let mut cols: Vec<Column> = (0..3).map(|i| col(&format!("C{i}"), "VARCHAR2")).collect();
        let range = || PageRange::Rowid { lo: None, hi: None };
        let new = digest_sql(&dialect("23.26.3.0.0"), "A", "T", &cols, range(), None).unwrap();
        assert!(new.text.contains("LENGTHB(\"C0\")"));
        assert!(new.text.contains("||':'||\"C0\",'n')||CHR(31)||"));
        assert!(new.text.contains("SUBSTR(H,46,15)"));
        let old = digest_sql(&dialect("11.2.0.2.0"), "A", "T", &cols, range(), None).unwrap();
        assert!(!old.text.contains("STANDARD_HASH"));
        assert!(old.text.contains("ORA_HASH(R,4294967295,4)"));
        for c in &mut cols {
            c.data_length = 3000;
        }
        let wide = digest_sql(&dialect("19.3.0.0.0"), "A", "T", &cols, range(), None).unwrap();
        assert_eq!(wide.text.matches("STANDARD_HASH(").count(), 4);
        cols[0].data_length = 4000;
        assert!(digest_sql(&dialect("19.3.0.0.0"), "A", "T", &cols, range(), None).is_err());
    }

    #[test]
    fn physical_digest_keeps_snapshot_bind_before_rowid_bounds() {
        let sql = digest_sql(
            &dialect("11.2.0.2.0"),
            "APP",
            "T",
            &[col("A", "VARCHAR2")],
            PageRange::Rowid {
                lo: Some("AAA"),
                hi: Some("BBB"),
            },
            Some(42),
        )
        .unwrap();
        assert!(
            sql.text
                .contains("AS OF SCN :b1 WHERE ROWID >= CHARTOROWID(:b2)")
        );
        assert_eq!(
            sql.binds,
            [
                Bind::Number("42".into()),
                Bind::Text("AAA".into()),
                Bind::Text("BBB".into())
            ]
        );
    }

    #[test]
    fn lob_digest_pages_keep_locators_snapshot_bounds_and_overflow_probe() {
        let cols = vec![
            col("ID", "NUMBER"),
            col("C", "CLOB"),
            col("N", "NCLOB"),
            col("B", "BLOB"),
        ];
        let keys = [&cols[0]];
        let lo = [KeyValue::Int(10)];
        let hi = [KeyValue::Int(20)];
        for version in ["11.2.0.2.0", "21.3.0.0.0", "23.5.0.24.07"] {
            let sql = page_sql(
                &dialect(version),
                "APP",
                "T",
                &cols,
                PageRange::Keyset {
                    keys: &keys,
                    lower: Some(Bound {
                        values: &lo,
                        inclusive: true,
                    }),
                    upper: Some(Bound {
                        values: &hi,
                        inclusive: false,
                    }),
                },
                42,
                100_001,
            )
            .unwrap();
            assert!(sql.text.contains(",\"C\",\"N\",\"B\" FROM"));
            assert!(sql.text.contains("AS OF SCN :b1"));
            assert!(sql.text.contains("\"ID\" >= :b2"));
            assert!(sql.text.contains("\"ID\" < :b3"));
            assert!(sql.text.contains("100001"));
            assert!(!sql.text.contains("HASH"));
            assert_eq!(
                sql.binds,
                [
                    Bind::Number("42".into()),
                    Bind::Number("10".into()),
                    Bind::Number("20".into()),
                ]
            );
        }
    }

    #[test]
    fn extents_group_and_bigfile_rowid_parts() {
        let e = |block_id, blocks| Extent {
            data_object_id: 7,
            relative_fno: 12,
            block_id,
            blocks,
        };
        let extents = [e(8, 8), e(16, 8), e(128, 128), e(256, 128), e(1024, 1024)];
        assert_eq!(group_extents(&extents, 128), vec![0, 3, 4]);
        assert_eq!(group_extents(&extents, 1), vec![0, 1, 2, 3, 4]);
        assert_eq!(rowid_parts(&e(99, 8)), (12, 99));
        let big = Extent {
            relative_fno: 1024,
            block_id: (5 << 22) | 17,
            ..e(0, 8)
        };
        assert_eq!(rowid_parts(&big), (5, 17));
        let split = split_extents(&[e(8, 25)], 10);
        assert_eq!(split, [e(8, 10), e(18, 10), e(28, 5)]);
        assert_eq!(split.iter().map(|e| e.blocks).sum::<u64>(), 25);
    }

    #[test]
    fn key_values_round_trip_canonical_text() {
        assert_eq!(key_value(ColKind::Number, "42").unwrap(), KeyValue::Int(42));
        assert_eq!(
            key_value(ColKind::Number, "1.50").unwrap(),
            KeyValue::Decimal("1.5".into())
        );
        assert_eq!(
            key_value(ColKind::Raw, "0A").unwrap(),
            KeyValue::Bytes("0a".into())
        );
        assert_eq!(
            key_value(ColKind::Char, "a  ").unwrap(),
            KeyValue::Text("a  ".into())
        );
    }
}
