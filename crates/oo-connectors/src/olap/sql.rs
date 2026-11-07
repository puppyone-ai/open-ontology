//! SQL text for the OLAP engines: identifier quoting, typed literals, the
//! lexicographic half-open range predicate and the statement guard.
//!
//! Bounds are compared by the source. A bound may be a prefix of the order
//! columns: `[p]` as a lower bound means "order column 0 is at least `p`".
//! NULL orders before every value, so `lo` excludes NULL and `hi` includes it,
//! keeping every row in exactly one chunk.

use crate::SourceError;
use crate::engine::{EngineFamily, KeyValue};

type Result<T> = std::result::Result<T, SourceError>;

/// How a bound value of one order column is compared by the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyKind {
    Int,
    Decimal,
    Text,
    /// Raw bytes; ClickHouse `String` that is not printable UTF-8.
    Bytes,
}

/// One keyset column. `expr` is the SQL the source compares and orders by;
/// `sample` renders a boundary value as text that `literal` turns back into
/// the same value; `cast` is the source type of a literal (empty for none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OrderCol {
    pub name: String,
    pub expr: String,
    pub sample: String,
    pub kind: KeyKind,
    pub cast: String,
}

/// Backtick-quote one identifier. Names with quotes, backslashes or control
/// characters are refused rather than escaped.
pub(crate) fn ident(name: &str) -> Result<String> {
    if name.is_empty()
        || name.len() > 192
        || name
            .chars()
            .any(|c| c == '`' || c == '\\' || c == '\'' || c == '"' || c.is_control())
    {
        return Err(SourceError::identifier("olap_identifier"));
    }
    Ok(format!("`{name}`"))
}

pub(crate) fn table_sql(schema: &str, name: &str) -> Result<String> {
    Ok(format!("{}.{}", ident(schema)?, ident(name)?))
}

/// Single-quoted string literal for metadata lookups.
pub(crate) fn text_literal(value: &str) -> Result<String> {
    if value.chars().any(|c| c.is_control()) {
        return Err(SourceError::identifier("olap_literal_control"));
    }
    Ok(format!(
        "'{}'",
        value.replace('\\', "\\\\").replace('\'', "\\'")
    ))
}

fn decimal_text_ok(text: &str) -> bool {
    let body = text.strip_prefix('-').unwrap_or(text);
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    !int.is_empty()
        && int.bytes().all(|b| b.is_ascii_digit())
        && frac.bytes().all(|b| b.is_ascii_digit())
        && !body.ends_with('.')
        && text.len() <= 100
}

/// Typed literal of one bound value, refused when it does not fit the column.
pub(crate) fn literal(col: &OrderCol, value: &KeyValue) -> Result<String> {
    let typed = |quoted: String| {
        if col.cast.is_empty() {
            quoted
        } else {
            format!("CAST({quoted} AS {})", col.cast)
        }
    };
    match (col.kind, value) {
        (KeyKind::Int, KeyValue::Int(n)) => Ok(n.to_string()),
        (KeyKind::Decimal, KeyValue::Decimal(text)) if decimal_text_ok(text) => {
            Ok(typed(format!("'{text}'")))
        }
        (KeyKind::Text | KeyKind::Bytes, KeyValue::Text(text)) => Ok(typed(text_literal(text)?)),
        (KeyKind::Bytes, KeyValue::Bytes(hex))
            if !hex.is_empty()
                && hex.len() % 2 == 0
                && hex.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            Ok(typed(format!("unhex('{hex}')")))
        }
        _ => Err(SourceError::config("olap_bound_type")),
    }
}

fn null_false(family: EngineFamily, expr: String) -> String {
    match family {
        EngineFamily::Clickhouse => format!("ifNull({expr}, 0)"),
        _ => format!("IFNULL({expr}, FALSE)"),
    }
}

/// `(c1..cm) >= (b1..bm)` lexicographically, two-valued: NULL is "less".
fn at_least(family: EngineFamily, cols: &[OrderCol], bound: &[KeyValue]) -> Result<String> {
    if bound.is_empty() || bound.len() > cols.len() {
        return Err(SourceError::config("olap_bound_width"));
    }
    let lits = cols
        .iter()
        .zip(bound)
        .map(|(c, v)| literal(c, v))
        .collect::<Result<Vec<_>>>()?;
    let mut terms = Vec::new();
    for i in 0..bound.len() {
        let mut parts: Vec<String> = (0..i)
            .map(|j| format!("{} = {}", cols[j].expr, lits[j]))
            .collect();
        parts.push(format!("{} > {}", cols[i].expr, lits[i]));
        terms.push(format!("({})", parts.join(" AND ")));
    }
    let equal: Vec<String> = (0..bound.len())
        .map(|j| format!("{} = {}", cols[j].expr, lits[j]))
        .collect();
    terms.push(format!("({})", equal.join(" AND ")));
    Ok(null_false(family, format!("({})", terms.join(" OR "))))
}

/// Predicate for `[lo, hi)`, or `None` for the whole table.
pub(crate) fn range(
    family: EngineFamily,
    cols: &[OrderCol],
    lo: Option<&[KeyValue]>,
    hi: Option<&[KeyValue]>,
) -> Result<Option<String>> {
    let mut parts = Vec::new();
    if let Some(lo) = lo {
        parts.push(at_least(family, cols, lo)?);
    }
    if let Some(hi) = hi {
        parts.push(format!("NOT {}", at_least(family, cols, hi)?));
    }
    Ok((!parts.is_empty()).then(|| parts.join(" AND ")))
}

/// Every statement the engine sends. Reads, metadata and the session
/// timeout only; anything else is a programming error and is refused.
pub(crate) fn guard(sql: &str) -> Result<()> {
    let head = sql.trim_start();
    let upper = head
        .get(..head.len().min(24))
        .unwrap_or("")
        .to_ascii_uppercase();
    let allowed = upper.starts_with("SELECT ")
        || upper.starts_with("SHOW ")
        || upper.starts_with("DESC ")
        || upper.starts_with("SET QUERY_TIMEOUT = ");
    // Literals are escaped by `literal`, so a `;` can only appear inside one.
    if !allowed {
        return Err(SourceError::protocol("olap_statement_guard"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, kind: KeyKind, cast: &str) -> OrderCol {
        OrderCol {
            name: name.into(),
            expr: format!("`{name}`"),
            sample: format!("`{name}`"),
            kind,
            cast: cast.into(),
        }
    }

    #[test]
    fn prefix_bounds_are_lexicographic_and_null_safe() {
        let cols = [col("d", KeyKind::Text, "date"), col("id", KeyKind::Int, "")];
        let lo = [KeyValue::Text("2024-02-01".into())];
        let hi = [KeyValue::Text("2024-02-01".into()), KeyValue::Int(7)];
        let sql = range(EngineFamily::Doris, &cols, Some(&lo), Some(&hi))
            .unwrap()
            .unwrap();
        assert_eq!(
            sql,
            "IFNULL(((`d` > CAST('2024-02-01' AS date)) OR (`d` = CAST('2024-02-01' AS date))), FALSE) \
             AND NOT IFNULL(((`d` > CAST('2024-02-01' AS date)) OR (`d` = CAST('2024-02-01' AS date) \
             AND `id` > 7) OR (`d` = CAST('2024-02-01' AS date) AND `id` = 7)), FALSE)"
        );
        assert_eq!(range(EngineFamily::Doris, &cols, None, None).unwrap(), None);
        let ch = range(
            EngineFamily::Clickhouse,
            &[col("_partition_id", KeyKind::Text, "")],
            None,
            Some(&[KeyValue::Text("202402".into())]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            ch,
            "NOT ifNull(((`_partition_id` > '202402') OR (`_partition_id` = '202402')), 0)"
        );
    }

    #[test]
    fn literals_are_typed_and_refuse_mismatch() {
        let s = col("s", KeyKind::Bytes, "String");
        assert_eq!(
            literal(&s, &KeyValue::Text("it's \\".into())).unwrap(),
            "CAST('it\\'s \\\\' AS String)"
        );
        assert_eq!(
            literal(&s, &KeyValue::Bytes("ff00".into())).unwrap(),
            "CAST(unhex('ff00') AS String)"
        );
        assert!(literal(&s, &KeyValue::Bytes("zz".into())).is_err());
        let n = col("n", KeyKind::Decimal, "decimal(12,2)");
        assert_eq!(
            literal(&n, &KeyValue::Decimal("-12.50".into())).unwrap(),
            "CAST('-12.50' AS decimal(12,2))"
        );
        for bad in ["1e5", "1.", ".5", "1-2", "' OR 1"] {
            assert!(
                literal(&n, &KeyValue::Decimal(bad.into())).is_err(),
                "{bad}"
            );
        }
        assert!(literal(&n, &KeyValue::Int(1)).is_err());
        assert!(literal(&s, &KeyValue::Text("a\nb".into())).is_err());
        assert!(range(EngineFamily::Doris, &[n], Some(&[KeyValue::Null]), None).is_err());
    }

    #[test]
    fn identifiers_and_guard_fail_closed() {
        assert_eq!(ident("events_2024").unwrap(), "`events_2024`");
        for bad in ["", "a`b", "a\\b", "a'b", "a\nb"] {
            assert!(ident(bad).is_err(), "{bad:?}");
        }
        for ok in [
            "SELECT 1",
            "show partitions from `a`.`b`",
            "DESC `a`.`b`",
            "SET query_timeout = 30",
        ] {
            guard(ok).unwrap();
        }
        for bad in [
            "INSERT INTO t VALUES (1)",
            "SET GLOBAL x = 1",
            "OPTIMIZE TABLE t",
            "ALTER TABLE t DELETE WHERE 1",
            "KILL QUERY 1",
        ] {
            assert!(guard(bad).is_err(), "{bad}");
        }
    }
}
