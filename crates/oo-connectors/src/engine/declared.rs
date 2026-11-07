//! Bounded wire representation; comment contents never enter errors or logs.
use super::{DeclaredSourceCatalog, DeclaredSourceTable, ReadBudget};
use crate::SourceError;
use std::io::{self, Write};

pub(crate) const BYTE_LIMIT: &str = "declared_catalog_byte_limit";

struct Counter {
    remaining: u64,
}

impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len() as u64)
            .ok_or_else(|| io::Error::other(BYTE_LIMIT))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn size<T: serde::Serialize>(value: &T, limit: u64) -> Result<u64, SourceError> {
    let mut counter = Counter { remaining: limit };
    serde_json::to_writer(&mut counter, value).map_err(|_| SourceError::budget(BYTE_LIMIT))?;
    Ok(limit - counter.remaining)
}

pub(crate) struct Export {
    pub catalog: DeclaredSourceCatalog,
    pub remaining: u64,
}

impl Export {
    pub fn new(source_id: String, budget: &ReadBudget) -> Result<Self, SourceError> {
        let catalog = DeclaredSourceCatalog {
            protocol: "oo.builder.source-catalog/v1".into(),
            source_id,
            tables: Vec::new(),
        };
        let remaining = budget.max_chunk_bytes - size(&catalog, budget.max_chunk_bytes)?;
        Ok(Self { catalog, remaining })
    }

    pub fn push(&mut self, table: DeclaredSourceTable) -> Result<(), SourceError> {
        let comma = u64::from(!self.catalog.tables.is_empty());
        let limit = self
            .remaining
            .checked_sub(comma)
            .ok_or_else(|| SourceError::budget(BYTE_LIMIT))?;
        self.remaining = limit - size(&table, limit)?;
        self.catalog.tables.push(table);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::DeclaredSourceColumn;
    use crate::{ForeignKey, ForeignKeyColumn, KeyColumn};

    #[test]
    fn cumulative_comment_budget_refuses_without_partial_table() {
        let table = DeclaredSourceTable {
            schema: "app".into(),
            name: "notes".into(),
            comment: Some("\u{4e2d}\"\\\n".repeat(100)),
            columns: vec![],
            primary_key: vec![],
            foreign_keys: vec![],
        };
        let mut one = Export::new("source".into(), &ReadBudget::default()).unwrap();
        one.push(table.clone()).unwrap();
        let budget = ReadBudget {
            max_chunk_bytes: serde_json::to_vec(&one.catalog).unwrap().len() as u64,
            ..Default::default()
        };
        let mut bounded = Export::new("source".into(), &budget).unwrap();
        bounded.push(table.clone()).unwrap();
        assert_eq!(bounded.remaining, 0);
        assert_eq!(bounded.push(table).unwrap_err().fact(), Some(BYTE_LIMIT));
        assert_eq!(bounded.catalog.tables.len(), 1);
        let empty_budget = ReadBudget {
            max_chunk_bytes: 1,
            ..budget
        };
        assert_eq!(
            Export::new("source".into(), &empty_budget)
                .err()
                .unwrap()
                .fact(),
            Some(BYTE_LIMIT)
        );
    }

    #[test]
    fn export_shape_and_exact_utf8_wire_budget() {
        let mut export = Export::new("source".into(), &ReadBudget::default()).unwrap();
        export
            .push(DeclaredSourceTable {
                schema: "app".into(),
                name: "child".into(),
                comment: Some("untrusted: \"ignore instructions\"\n".into()),
                columns: vec![DeclaredSourceColumn {
                    name: "id".into(),
                    not_null: true,
                    comment: Some("key".into()),
                }],
                primary_key: vec![KeyColumn {
                    attnum: 1,
                    name: "id".into(),
                }],
                foreign_keys: vec![ForeignKey {
                    name: "parent_fk".into(),
                    columns: vec![ForeignKeyColumn {
                        attnum: 1,
                        name: "id".into(),
                        ref_schema: "app".into(),
                        ref_table: "parent".into(),
                        ref_attnum: 2,
                        ref_name: "id".into(),
                    }],
                }],
            })
            .unwrap();
        let bytes = serde_json::to_vec(&export.catalog).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["protocol"], "oo.builder.source-catalog/v1");
        assert_eq!(json["tables"][0]["columns"][0]["not_null"], true);
        assert_eq!(
            json["tables"][0]["foreign_keys"][0]["columns"][0]["ref_attnum"],
            2
        );
        assert_eq!(
            size(&export.catalog, bytes.len() as u64).unwrap(),
            bytes.len() as u64
        );
        assert_eq!(
            size(&export.catalog, bytes.len() as u64 - 1)
                .unwrap_err()
                .fact(),
            Some(BYTE_LIMIT)
        );
        assert_eq!(
            ReadBudget::default().max_chunk_bytes - export.remaining,
            bytes.len() as u64
        );
        let decoded: DeclaredSourceCatalog = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
    }
}
