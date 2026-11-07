//! Pure planning: key choice, chunk assembly, and `EXPLAIN` reading.
//! Database round trips live in `mod.rs`; everything here is unit tested.

use serde_json::Value;

use crate::SourceError;
use crate::engine::{ChunkSpec, KeyShape, KeyValue, TableRef};

use super::sqltext::KeyClass;

/// PostgreSQL `MaxHeapTuplesPerPage` for 8 KiB blocks.
pub const MAX_TUPLES_PER_PAGE_8K: u64 = 291;

/// One column of a unique index candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexCol {
    pub name: String,
    pub not_null: bool,
    pub cast_type: String,
    /// Domain base type oid, or the type oid.
    pub base_oid: i64,
    /// `pg_type.typcategory` of the base type.
    pub category: String,
    pub collation: Option<String>,
}

/// A valid unique btree index of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexCandidate {
    pub name: String,
    pub primary: bool,
    /// Not partial, no expression column.
    pub plain: bool,
    pub columns: Vec<IndexCol>,
}

/// Pick the key: a plain primary key, else the narrowest plain unique index
/// whose columns are all `NOT NULL`, else no key.
pub fn choose_key(candidates: &[IndexCandidate]) -> (KeyShape, Option<IndexCandidate>) {
    let usable = |c: &&IndexCandidate| c.plain && !c.columns.is_empty();
    if let Some(pk) = candidates.iter().filter(usable).find(|c| c.primary) {
        let names: Vec<String> = pk.columns.iter().map(|c| c.name.clone()).collect();
        let shape = match pk.columns.as_slice() {
            [one] if KeyClass::from_oid(one.base_oid) == KeyClass::Int => {
                KeyShape::IntegerPrimaryKey {
                    column: one.name.clone(),
                }
            }
            [one] if one.category == "S" => KeyShape::StringPrimaryKey {
                column: one.name.clone(),
                collation: one.collation.clone(),
            },
            // Other single-column types (uuid, numeric, dates) use the
            // generic keyset shape until the interface grows a variant.
            _ => KeyShape::CompositePrimaryKey { columns: names },
        };
        return (shape, Some(pk.clone()));
    }
    let unique = candidates
        .iter()
        .filter(usable)
        .filter(|c| !c.primary && c.columns.iter().all(|col| col.not_null))
        .min_by(|a, b| {
            a.columns
                .len()
                .cmp(&b.columns.len())
                .then_with(|| a.name.cmp(&b.name))
        });
    match unique {
        Some(index) => (
            KeyShape::UniqueIndex {
                index: index.name.clone(),
                columns: index.columns.iter().map(|c| c.name.clone()).collect(),
            },
            Some(index.clone()),
        ),
        None => (KeyShape::NoKey, None),
    }
}

/// Rows targeted per planned chunk: half the row budget, so a chunk can grow
/// between planning and reading before it must split.
pub fn target_rows(max_chunk_rows: u64) -> u64 {
    (max_chunk_rows / 2).max(1)
}

/// `[None, b1), [b1, b2), ..., [bn, None)`. The open ends cover rows inserted
/// outside the planned range.
pub fn keyset_chunks(
    table: &TableRef,
    revision: u32,
    bounds: Vec<Vec<KeyValue>>,
    estimated: Option<u64>,
) -> Result<Vec<ChunkSpec>, SourceError> {
    let count = bounds.len() + 1;
    let per = estimated.map(|rows| rows.div_ceil(count as u64));
    let mut lo: Option<Vec<KeyValue>> = None;
    let mut out = Vec::with_capacity(count);
    let mut iter = bounds.into_iter();
    for id in 0..count {
        let hi = iter.next();
        out.push(ChunkSpec {
            table: table.clone(),
            plan_revision: revision,
            chunk_id: u32::try_from(id).map_err(|_| SourceError::budget("chunk_count"))?,
            lo: lo.clone(),
            hi: hi.clone(),
            estimated_rows: per,
        });
        lo = hi;
    }
    Ok(out)
}

pub fn tid(block: u64) -> KeyValue {
    KeyValue::Locator(format!("({block},0)"))
}

/// Block number of a `(block,0)` locator.
pub fn tid_block(value: &KeyValue) -> Result<u64, SourceError> {
    let KeyValue::Locator(text) = value else {
        return Err(SourceError::protocol("ctid_bound"));
    };
    text.strip_prefix('(')
        .and_then(|t| t.strip_suffix(",0)"))
        .and_then(|t| t.parse().ok())
        .ok_or_else(|| SourceError::protocol("ctid_bound"))
}

/// Pages per ctid chunk for `target` rows at `tuples_per_page`.
pub fn pages_per_chunk(target: u64, tuples_per_page: u64) -> u64 {
    (target / tuples_per_page.clamp(1, MAX_TUPLES_PER_PAGE_8K)).max(1)
}

/// ctid chunks over `blocks` pages: `[None,(p,0))`, ..., `[(q,0), None)`.
pub fn ctid_chunks(
    table: &TableRef,
    revision: u32,
    blocks: u64,
    tuples_per_page: u64,
    target: u64,
) -> Result<Vec<ChunkSpec>, SourceError> {
    let step = pages_per_chunk(target, tuples_per_page);
    let mut bounds = Vec::new();
    let mut next = step;
    while next < blocks {
        if bounds.len() >= 100_000 {
            return Err(SourceError::budget("chunk_count"));
        }
        bounds.push(vec![tid(next)]);
        next += step;
    }
    let per = Some(step.saturating_mul(tuples_per_page.max(1)));
    let mut chunks = keyset_chunks(table, revision, bounds, None)?;
    for chunk in &mut chunks {
        chunk.estimated_rows = per;
    }
    Ok(chunks)
}

/// Result of reading an `EXPLAIN (FORMAT JSON)` plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplainNote {
    /// Node type that scans the table, e.g. `Index Scan`.
    pub node: String,
    /// True when that node is an index scan of the key index with an index condition.
    pub key_index: bool,
}

/// Find the scan node of the plan and whether it uses `index` with a condition.
pub fn read_explain(json: &str, index: &str) -> Result<ExplainNote, SourceError> {
    let value: Value = serde_json::from_str(json).map_err(|_| SourceError::protocol("explain"))?;
    let plan = value
        .get(0)
        .and_then(|v| v.get("Plan"))
        .ok_or_else(|| SourceError::protocol("explain"))?;
    let mut found = None;
    walk(plan, index, &mut found);
    Ok(found.unwrap_or(ExplainNote {
        node: "unknown".into(),
        key_index: false,
    }))
}

fn walk(node: &Value, index: &str, found: &mut Option<ExplainNote>) {
    let kind = node.get("Node Type").and_then(Value::as_str).unwrap_or("");
    if kind.ends_with("Scan") && found.is_none() {
        let key_index = matches!(kind, "Index Scan" | "Index Only Scan")
            && node.get("Index Name").and_then(Value::as_str) == Some(index)
            && node.get("Index Cond").is_some();
        *found = Some(ExplainNote {
            node: kind.to_owned(),
            key_index,
        });
    }
    if let Some(children) = node.get("Plans").and_then(Value::as_array) {
        for child in children {
            walk(child, index, found);
        }
    }
}

/// `Plan Rows` of the top node.
pub fn explain_rows(json: &str) -> Result<u64, SourceError> {
    let value: Value = serde_json::from_str(json).map_err(|_| SourceError::protocol("explain"))?;
    value
        .get(0)
        .and_then(|v| v.get("Plan"))
        .and_then(|p| p.get("Plan Rows"))
        .and_then(Value::as_f64)
        .map(|rows| rows.max(0.0) as u64)
        .ok_or_else(|| SourceError::protocol("explain"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, oid: i64, category: &str, not_null: bool) -> IndexCol {
        IndexCol {
            name: name.into(),
            not_null,
            cast_type: "t".into(),
            base_oid: oid,
            category: category.into(),
            collation: (category == "S").then(|| "en-x-icu".into()),
        }
    }

    fn idx(name: &str, primary: bool, plain: bool, columns: Vec<IndexCol>) -> IndexCandidate {
        IndexCandidate {
            name: name.into(),
            primary,
            plain,
            columns,
        }
    }

    fn table() -> TableRef {
        TableRef {
            schema: "s".into(),
            name: "t".into(),
        }
    }

    #[test]
    fn key_shapes() {
        let int = idx("pk", true, true, vec![col("id", 20, "N", true)]);
        assert!(matches!(
            choose_key(&[int]).0,
            KeyShape::IntegerPrimaryKey { column } if column == "id"
        ));
        let text = idx("pk", true, true, vec![col("code", 1043, "S", true)]);
        assert_eq!(
            choose_key(&[text]).0,
            KeyShape::StringPrimaryKey {
                column: "code".into(),
                collation: Some("en-x-icu".into())
            }
        );
        let comp = idx(
            "pk",
            true,
            true,
            vec![col("region", 23, "N", true), col("code", 25, "S", true)],
        );
        assert!(
            matches!(choose_key(&[comp]).0, KeyShape::CompositePrimaryKey { columns } if columns.len() == 2)
        );
        let uuid = idx("pk", true, true, vec![col("id", 2950, "U", true)]);
        assert!(
            matches!(choose_key(&[uuid]).0, KeyShape::CompositePrimaryKey { columns } if columns == ["id"])
        );
    }

    #[test]
    fn unique_needs_not_null_and_plain() {
        let nullable = idx("u_alt", false, true, vec![col("alt", 25, "S", false)]);
        let partial = idx("u_part", false, false, vec![col("p", 20, "N", true)]);
        let wide = idx(
            "u_wide",
            false,
            true,
            vec![col("a", 20, "N", true), col("b", 20, "N", true)],
        );
        let narrow = idx("u_ext", false, true, vec![col("ext", 20, "N", true)]);
        let (shape, chosen) = choose_key(&[nullable.clone(), partial.clone(), wide, narrow]);
        assert_eq!(
            shape,
            KeyShape::UniqueIndex {
                index: "u_ext".into(),
                columns: vec!["ext".into()]
            }
        );
        assert_eq!(chosen.unwrap().name, "u_ext");
        assert_eq!(choose_key(&[nullable, partial]).0, KeyShape::NoKey);
        assert_eq!(choose_key(&[]).0, KeyShape::NoKey);
        // A non-plain primary key falls through to unique indexes.
        let pk_expr = idx("pk", true, false, vec![col("x", 20, "N", true)]);
        let u = idx("u", false, true, vec![col("y", 20, "N", true)]);
        assert!(matches!(
            choose_key(&[pk_expr, u]).0,
            KeyShape::UniqueIndex { .. }
        ));
    }

    #[test]
    fn keyset_chunks_are_half_open_and_cover_both_ends() {
        let bounds = vec![vec![KeyValue::Int(10)], vec![KeyValue::Int(20)]];
        let chunks = keyset_chunks(&table(), 3, bounds, Some(30)).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].lo, None);
        assert_eq!(chunks[0].hi, Some(vec![KeyValue::Int(10)]));
        assert_eq!(chunks[1].lo, chunks[0].hi);
        assert_eq!(chunks[2].hi, None);
        assert!(chunks.iter().all(|c| c.plan_revision == 3));
        assert_eq!(chunks[2].chunk_id, 2);
        assert_eq!(chunks[0].estimated_rows, Some(10));
        let single = keyset_chunks(&table(), 1, vec![], None).unwrap();
        assert_eq!((single[0].lo.clone(), single[0].hi.clone()), (None, None));
    }

    #[test]
    fn ctid_ranges() {
        let chunks = ctid_chunks(&table(), 1, 1_000, 100, 25_000).unwrap();
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[1].lo, Some(vec![tid(250)]));
        assert_eq!(tid_block(&tid(750)).unwrap(), 750);
        assert!(tid_block(&KeyValue::Int(1)).is_err());
        assert_eq!(pages_per_chunk(10, 1_000), 1);
        assert_eq!(ctid_chunks(&table(), 1, 0, 0, 10).unwrap().len(), 1);
    }

    #[test]
    fn explain_reading() {
        let json = r#"[{"Plan":{"Node Type":"Sort","Plans":[{"Node Type":"Index Scan","Index Name":"t_pkey","Index Cond":"((a, b) >= (1, 'x'))","Plan Rows":12}]}}]"#;
        assert_eq!(
            read_explain(json, "t_pkey").unwrap(),
            ExplainNote {
                node: "Index Scan".into(),
                key_index: true
            }
        );
        assert!(!read_explain(json, "other").unwrap().key_index);
        let seq = r#"[{"Plan":{"Node Type":"Seq Scan","Plan Rows":2000000.0}}]"#;
        assert!(!read_explain(seq, "t_pkey").unwrap().key_index);
        assert_eq!(explain_rows(seq).unwrap(), 2_000_000);
        assert!(read_explain("{}", "x").is_err());
    }
}
