//! SQL text for the adapter. Identifiers are `quote_ident` output from the
//! server; values are always bound parameters cast to the column's own type
//! (`format_type`), so comparisons run in PostgreSQL under the column
//! collation. Nothing here appends `COLLATE`, `LIMIT` to a business read, or
//! `OFFSET` to a business read.

use crate::SourceError;
use crate::engine::KeyValue;
use crate::types::valid_quoted_ident;

/// How a key value is carried as text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyClass {
    Int,
    Decimal,
    Bytes,
    Text,
}

impl KeyClass {
    /// From the (domain base) type oid.
    pub fn from_oid(oid: i64) -> Self {
        match oid {
            20 | 21 | 23 => Self::Int,
            1700 => Self::Decimal,
            17 => Self::Bytes,
            _ => Self::Text,
        }
    }

    /// Server `::text` output to a lossless key value.
    pub fn value(self, text: Option<String>) -> Result<KeyValue, SourceError> {
        let Some(text) = text else {
            return Ok(KeyValue::Null);
        };
        Ok(match self {
            Self::Int => KeyValue::Int(
                text.parse()
                    .map_err(|_| SourceError::protocol("key_value"))?,
            ),
            Self::Decimal => KeyValue::Decimal(text),
            Self::Bytes => KeyValue::Bytes(
                text.strip_prefix("\\x")
                    .ok_or_else(|| SourceError::protocol("key_value"))?
                    .to_owned(),
            ),
            Self::Text => KeyValue::Text(text),
        })
    }
}

/// Text bound for one key value; the SQL casts it to the column type.
pub fn bind_text(value: &KeyValue) -> Result<String, SourceError> {
    Ok(match value {
        KeyValue::Null => return Err(SourceError::protocol("null_bound")),
        KeyValue::Int(v) => v.to_string(),
        KeyValue::Decimal(v) | KeyValue::Text(v) | KeyValue::Locator(v) => v.clone(),
        KeyValue::Bytes(hex) => format!("\\x{hex}"),
    })
}

/// One key column of the chosen key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyCol {
    pub name: String,
    pub quoted: String,
    /// `format_type(atttypid, atttypmod)`, used as `CAST($n AS <type>)`.
    pub cast_type: String,
    pub class: KeyClass,
    pub collation: Option<String>,
}

/// Production uses the EXPLAIN-verified row comparison. The expanded form is
/// retained only as a test oracle for the inclusive/exclusive boundary rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareForm {
    Row,
    #[cfg(test)]
    Expanded,
}

/// `format_type` output accepted as a cast target.
pub fn valid_cast_type(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 256
        && !text.contains(';')
        && !text.contains("--")
        && !text.contains("/*")
        && !text.contains('\0')
        && !text.contains('$')
        && !text.contains('\'')
}

pub fn check_idents<'a>(idents: impl IntoIterator<Item = &'a str>) -> Result<(), SourceError> {
    for ident in idents {
        if !valid_quoted_ident(ident) {
            return Err(SourceError::catalog("quote_ident"));
        }
    }
    Ok(())
}

fn param(keys: &[KeyCol], index: usize, first: usize) -> String {
    format!("CAST(${} AS {})", first + index, keys[index].cast_type)
}

/// `(k) >= (lo)` or `(k) < (hi)` with parameters starting at `first`.
/// Returns the predicate; it uses `keys.len()` parameters.
pub fn bound(keys: &[KeyCol], form: CompareForm, lower: bool, first: usize) -> String {
    let op_last = if lower { ">=" } else { "<" };
    if keys.len() == 1 {
        return format!("{} {op_last} {}", keys[0].quoted, param(keys, 0, first));
    }
    match form {
        CompareForm::Row => {
            let cols: Vec<&str> = keys.iter().map(|k| k.quoted.as_str()).collect();
            let params: Vec<String> = (0..keys.len()).map(|i| param(keys, i, first)).collect();
            format!("({}) {op_last} ({})", cols.join(","), params.join(","))
        }
        #[cfg(test)]
        CompareForm::Expanded => {
            let op_strict = if lower { ">" } else { "<" };
            // lower: OR_i (k0=v0 .. k_{i-1}=v_{i-1} AND k_i > v_i) OR (all equal)
            // upper: OR_i (k0=v0 .. k_{i-1}=v_{i-1} AND k_i < v_i)
            let mut terms = Vec::new();
            for i in 0..keys.len() {
                let mut parts: Vec<String> = (0..i)
                    .map(|j| format!("{} = {}", keys[j].quoted, param(keys, j, first)))
                    .collect();
                parts.push(format!(
                    "{} {op_strict} {}",
                    keys[i].quoted,
                    param(keys, i, first)
                ));
                terms.push(format!("({})", parts.join(" AND ")));
            }
            if lower {
                let all: Vec<String> = (0..keys.len())
                    .map(|j| format!("{} = {}", keys[j].quoted, param(keys, j, first)))
                    .collect();
                terms.push(format!("({})", all.join(" AND ")));
            }
            format!("({})", terms.join(" OR "))
        }
    }
}

/// Where clause for `[lo, hi)`; returns `(clause, parameter count)`.
pub fn range_where(keys: &[KeyCol], form: CompareForm, lo: bool, hi: bool) -> (String, usize) {
    let mut parts = Vec::new();
    let mut next = 1;
    if lo {
        parts.push(bound(keys, form, true, next));
        next += keys.len();
    }
    if hi {
        parts.push(bound(keys, form, false, next));
        next += keys.len();
    }
    if parts.is_empty() {
        (String::new(), 0)
    } else {
        (format!(" WHERE {}", parts.join(" AND ")), next - 1)
    }
}

/// ctid range `[lo, hi)` inside one snapshot.
pub fn ctid_where(lo: bool, hi: bool) -> (String, usize) {
    match (lo, hi) {
        (false, false) => (String::new(), 0),
        (true, false) => (" WHERE ctid >= CAST($1 AS tid)".into(), 1),
        (false, true) => (" WHERE ctid < CAST($1 AS tid)".into(), 1),
        (true, true) => (
            " WHERE ctid >= CAST($1 AS tid) AND ctid < CAST($2 AS tid)".into(),
            2,
        ),
    }
}

fn order_by(keys: &[KeyCol]) -> String {
    let cols: Vec<&str> = keys.iter().map(|k| k.quoted.as_str()).collect();
    format!(" ORDER BY {}", cols.join(","))
}

/// Business read of one chunk. An empty column list reads row presence only.
pub fn chunk_select(rel: &str, columns: &[String], where_sql: &str, keys: &[KeyCol]) -> String {
    let list = if columns.is_empty() {
        "1".to_owned()
    } else {
        columns.join(",")
    };
    let order = if keys.is_empty() {
        String::new()
    } else {
        order_by(keys)
    };
    format!("SELECT {list} FROM {rel}{where_sql}{order}")
}

pub const CURSOR: &str = "oo_chunk";

pub fn declare(select: &str) -> String {
    format!("DECLARE {CURSOR} NO SCROLL CURSOR FOR {select}")
}

pub fn fetch(rows: u32) -> String {
    format!("FETCH FORWARD {rows} FROM {CURSOR}")
}

pub fn close() -> String {
    format!("CLOSE {CURSOR}")
}

/// Key text of the row `offset` rows after `lo` (planning probe, returns one row).
pub fn boundary_probe(rel: &str, keys: &[KeyCol], form: CompareForm, lo: bool) -> String {
    bounded_boundary_probe(rel, keys, form, lo, false)
}

/// Split probes must stay within both parent bounds, using source collation.
pub fn bounded_boundary_probe(
    rel: &str,
    keys: &[KeyCol],
    form: CompareForm,
    lo: bool,
    hi: bool,
) -> String {
    let cols: Vec<String> = keys.iter().map(|k| format!("{}::text", k.quoted)).collect();
    // Unqualified ORDER BY would resolve the ::text output alias, sorting
    // integer keys lexicographically instead of by the source column type.
    let order: Vec<String> = keys
        .iter()
        .map(|k| format!("oo_source.{}", k.quoted))
        .collect();
    let (where_sql, used) = range_where(keys, form, lo, hi);
    format!(
        "SELECT {} FROM {rel} AS oo_source{where_sql} ORDER BY {} OFFSET ${} LIMIT 1",
        cols.join(","),
        order.join(","),
        used + 1
    )
}

/// Canonical text of one row: per column `N` for NULL or
/// `V<octet length>:<text>`, joined by `|`. The prefix makes NULL and the
/// empty string distinct and the encoding unambiguous.
pub fn row_text(columns: &[String]) -> String {
    if columns.is_empty() {
        return "''".to_owned();
    }
    let cells: Vec<String> = columns
        .iter()
        .map(|c| {
            format!(
                "CASE WHEN {c} IS NULL THEN 'N' ELSE 'V' || pg_catalog.octet_length({c}::text)::text || ':' || {c}::text END"
            )
        })
        .collect();
    cells.join(" || '|' || ")
}

/// Count plus two order-independent 64-bit sums over per-row sha256.
pub fn digest_select(rel: &str, columns: &[String], where_sql: &str) -> String {
    let h = format!(
        "pg_catalog.sha256(pg_catalog.convert_to({}, 'UTF8'))",
        row_text(columns)
    );
    format!(
        "SELECT pg_catalog.count(*)::int8, \
         COALESCE(pg_catalog.sum(('x' || pg_catalog.encode(pg_catalog.substring(d, 1, 8), 'hex'))::bit(64)::int8::numeric), 0)::text, \
         COALESCE(pg_catalog.sum(('x' || pg_catalog.encode(pg_catalog.substring(d, 9, 8), 'hex'))::bit(64)::int8::numeric), 0)::text \
         FROM (SELECT {h} AS d FROM {rel}{where_sql}) AS oo_rows"
    )
}

/// Statements the adapter may send. Used by the read-only test below.
#[cfg(test)]
pub fn read_only(sql: &str) -> bool {
    let upper = sql.to_ascii_uppercase();
    let first = upper.split_whitespace().next().unwrap_or("");
    let allowed = matches!(
        first,
        "SELECT" | "DECLARE" | "FETCH" | "CLOSE" | "EXPLAIN" | "START" | "ROLLBACK"
    );
    let forbidden = [
        "INSERT ",
        "UPDATE ",
        "DELETE ",
        "TRUNCATE",
        "CREATE ",
        "ALTER ",
        "DROP ",
        "GRANT ",
        "LOCK ",
        "COPY ",
        "FOR UPDATE",
        "FOR SHARE",
        "FOR NO KEY",
        "FOR KEY SHARE",
    ];
    allowed && !forbidden.iter().any(|f| upper.contains(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str, ty: &str, class: KeyClass) -> KeyCol {
        KeyCol {
            name: name.into(),
            quoted: name.into(),
            cast_type: ty.into(),
            class,
            collation: None,
        }
    }

    #[test]
    fn split_probe_binds_both_parent_bounds_before_the_offset() {
        let keys = [
            key("a", "bigint", KeyClass::Int),
            key("b", "text", KeyClass::Text),
        ];
        let sql = bounded_boundary_probe("fixture.growing", &keys, CompareForm::Row, true, true);
        assert!(sql.contains("(a,b) >= (CAST($1 AS bigint),CAST($2 AS text))"));
        assert!(sql.contains("(a,b) < (CAST($3 AS bigint),CAST($4 AS text))"));
        assert!(sql.ends_with("ORDER BY oo_source.a,oo_source.b OFFSET $5 LIMIT 1"));
        assert!(read_only(&sql));
    }

    #[test]
    fn single_key_range() {
        let k = [key("id", "bigint", KeyClass::Int)];
        let (w, n) = range_where(&k, CompareForm::Row, true, true);
        assert_eq!(
            w,
            " WHERE id >= CAST($1 AS bigint) AND id < CAST($2 AS bigint)"
        );
        assert_eq!(n, 2);
        assert_eq!(
            range_where(&k, CompareForm::Row, false, false),
            (String::new(), 0)
        );
        let (w, n) = range_where(&k, CompareForm::Row, false, true);
        assert_eq!((w.as_str(), n), (" WHERE id < CAST($1 AS bigint)", 1));
    }

    #[test]
    fn composite_row_and_expanded_forms() {
        let k = [
            key("region", "integer", KeyClass::Int),
            key("code", "character varying(40)", KeyClass::Text),
        ];
        let (w, n) = range_where(&k, CompareForm::Row, true, true);
        assert_eq!(
            w,
            " WHERE (region,code) >= (CAST($1 AS integer),CAST($2 AS character varying(40))) \
             AND (region,code) < (CAST($3 AS integer),CAST($4 AS character varying(40)))"
        );
        assert_eq!(n, 4);
        let lower = bound(&k, CompareForm::Expanded, true, 1);
        assert_eq!(
            lower,
            "((region > CAST($1 AS integer)) OR (region = CAST($1 AS integer) AND code > \
             CAST($2 AS character varying(40))) OR (region = CAST($1 AS integer) AND code = \
             CAST($2 AS character varying(40))))"
        );
        let upper = bound(&k, CompareForm::Expanded, false, 3);
        assert_eq!(
            upper,
            "((region < CAST($3 AS integer)) OR (region = CAST($3 AS integer) AND code < \
             CAST($4 AS character varying(40))))"
        );
    }

    #[test]
    fn chunk_sql_never_limits_or_collates() {
        let k = [key("code", "text", KeyClass::Text)];
        let (w, _) = range_where(&k, CompareForm::Row, true, false);
        let sql = chunk_select("s.t", &["code".into(), "v".into()], &w, &k);
        assert_eq!(
            sql,
            "SELECT code,v FROM s.t WHERE code >= CAST($1 AS text) ORDER BY code"
        );
        for bad in ["LIMIT", "OFFSET", "COLLATE"] {
            assert!(!sql.contains(bad));
        }
        assert!(read_only(&declare(&sql)));
        let (w, n) = ctid_where(true, true);
        assert_eq!(n, 2);
        assert_eq!(
            chunk_select("s.t", &[], &w, &[]),
            "SELECT 1 FROM s.t WHERE ctid >= CAST($1 AS tid) AND ctid < CAST($2 AS tid)"
        );
    }

    #[test]
    fn probe_and_digest_text() {
        let k = [key("id", "bigint", KeyClass::Int)];
        assert_eq!(
            boundary_probe("s.t", &k, CompareForm::Row, true),
            "SELECT id::text FROM s.t AS oo_source WHERE id >= CAST($1 AS bigint) ORDER BY oo_source.id OFFSET $2 LIMIT 1"
        );
        assert_eq!(
            boundary_probe("s.t", &k, CompareForm::Row, false),
            "SELECT id::text FROM s.t AS oo_source ORDER BY oo_source.id OFFSET $1 LIMIT 1"
        );
        let d = digest_select("s.t", &["a".into(), "b".into()], "");
        assert!(d.contains("CASE WHEN a IS NULL THEN 'N'"));
        assert!(d.contains("|| '|' ||"));
        for sql in [d, fetch(4096), close()] {
            assert!(read_only(&sql), "{sql}");
        }
        assert!(!read_only("SELECT 1 FROM t FOR UPDATE"));
        assert!(!read_only("CREATE TABLE x()"));
    }

    #[test]
    fn key_values_round_trip_as_text() {
        assert_eq!(
            KeyClass::Int.value(Some("-42".into())).unwrap(),
            KeyValue::Int(-42)
        );
        assert_eq!(
            KeyClass::Bytes.value(Some("\\x00ff".into())).unwrap(),
            KeyValue::Bytes("00ff".into())
        );
        assert_eq!(
            bind_text(&KeyValue::Bytes("00ff".into())).unwrap(),
            "\\x00ff"
        );
        assert_eq!(KeyClass::Text.value(None).unwrap(), KeyValue::Null);
        assert!(bind_text(&KeyValue::Null).is_err());
        assert!(KeyClass::Int.value(Some("x".into())).is_err());
        assert_eq!(KeyClass::from_oid(20), KeyClass::Int);
        assert_eq!(KeyClass::from_oid(1700), KeyClass::Decimal);
        assert_eq!(KeyClass::from_oid(25), KeyClass::Text);
        assert!(valid_cast_type("character varying(40)"));
        assert!(valid_cast_type("\"MySchema\".\"Code\""));
        assert!(!valid_cast_type("int); DROP TABLE x; --"));
    }
}
