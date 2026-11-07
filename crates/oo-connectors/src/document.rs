//! Lossless document envelope shared by the document source engines.
//! Scalar projections are explicit: sampling cannot establish a schema for a
//! schemaless collection. Missing fields are NULL; the envelope preserves absence.

use std::{collections::BTreeSet, sync::Arc, time::Duration};

use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{SourceError, engine::ReadBudget};

pub(crate) type Result<T> = std::result::Result<T, SourceError>;
pub const DOCUMENT_METADATA: &str = "oo.document";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScalarType {
    Int64,
    Float64,
    Bool,
    Text,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScalarColumn {
    pub name: String,
    pub kind: ScalarType,
}

pub fn validate_columns(columns: &[ScalarColumn]) -> Result<()> {
    let mut names = BTreeSet::from(["_id", "_document"]);
    if columns.len() > 300 {
        return Err(SourceError::config("document_column_budget"));
    }
    for c in columns {
        if c.name.is_empty()
            || c.name.len() > 63
            || c.name.contains('\0')
            || !names.insert(c.name.as_str())
        {
            return Err(SourceError::config("document_column_name"));
        }
    }
    Ok(())
}

pub(crate) fn validate_budget(b: &ReadBudget) -> Result<()> {
    if !(1..=100_000).contains(&b.max_chunk_rows)
        || !(1..=32 * 1024 * 1024).contains(&b.max_chunk_bytes)
        || !(1..=100).contains(&b.duty_cycle_percent)
        || !(1..=300_000).contains(&b.statement_timeout_ms)
    {
        return Err(SourceError::config("document_budget"));
    }
    Ok(())
}

/// Arrow UTF-8 metadata is a target contract, not a cast of arbitrary strings.
pub(crate) fn batch(columns: &[ScalarColumn], rows: &[(String, Value)]) -> Result<RecordBatch> {
    validate_columns(columns)?;
    let mut fields = vec![Field::new("_id", DataType::Utf8, false)];
    let mut arrays: Vec<ArrayRef> = vec![Arc::new(StringArray::from_iter_values(
        rows.iter().map(|(id, _)| id.as_str()),
    ))];
    for column in columns {
        let values: Vec<_> = rows.iter().map(|(_, doc)| doc.get(&column.name)).collect();
        let mismatch = || SourceError::protocol("document_scalar_type_changed_rebaseline");
        let (kind, array): (DataType, ArrayRef) = match column.kind {
            ScalarType::Int64 => {
                let values = values
                    .iter()
                    .map(|v| match v {
                        None | Some(Value::Null) => Ok(None),
                        Some(v) => v
                            .as_i64()
                            .or_else(|| {
                                v.get("$numberLong")
                                    .or_else(|| v.get("$numberInt"))?
                                    .as_str()?
                                    .parse()
                                    .ok()
                            })
                            .map(Some)
                            .ok_or_else(mismatch),
                    })
                    .collect::<Result<Vec<_>>>()?;
                (DataType::Int64, Arc::new(Int64Array::from(values)))
            }
            ScalarType::Float64 => {
                let values = values
                    .iter()
                    .map(|v| match v {
                        None | Some(Value::Null) => Ok(None),
                        Some(v) => v
                            .as_f64()
                            .filter(|_| {
                                v.is_f64()
                                    || v.as_i64().is_some_and(|n| n.unsigned_abs() <= 1 << 53)
                            })
                            .or_else(|| v.get("$numberDouble")?.as_str()?.parse().ok())
                            .filter(|v| v.is_finite())
                            .map(Some)
                            .ok_or_else(mismatch),
                    })
                    .collect::<Result<Vec<_>>>()?;
                (DataType::Float64, Arc::new(Float64Array::from(values)))
            }
            ScalarType::Bool => {
                let values = values
                    .iter()
                    .map(|v| match v {
                        None | Some(Value::Null) => Ok(None),
                        Some(v) => v.as_bool().map(Some).ok_or_else(mismatch),
                    })
                    .collect::<Result<Vec<_>>>()?;
                (DataType::Boolean, Arc::new(BooleanArray::from(values)))
            }
            ScalarType::Text => {
                let values = values
                    .iter()
                    .map(|v| match v {
                        None | Some(Value::Null) => Ok(None),
                        Some(v) => v.as_str().map(Some).ok_or_else(mismatch),
                    })
                    .collect::<Result<Vec<_>>>()?;
                (DataType::Utf8, Arc::new(StringArray::from(values)))
            }
        };
        fields.push(Field::new(&column.name, kind, true));
        arrays.push(array);
    }
    fields.push(
        Field::new("_document", DataType::Utf8, false)
            .with_metadata([(DOCUMENT_METADATA.to_owned(), "jsonb".to_owned())]),
    );
    let docs: Vec<_> = rows.iter().map(|(_, doc)| doc.to_string()).collect();
    arrays.push(Arc::new(StringArray::from(docs)));
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
        .map_err(|_| SourceError::protocol("document_arrow"))
}

pub(crate) fn ipc(batch: &RecordBatch, budget: &ReadBudget) -> Result<(u64, String)> {
    if batch.num_rows() as u64 > budget.max_chunk_rows {
        return Err(SourceError::budget("document_chunk"));
    }
    let mut writer = arrow_ipc::writer::StreamWriter::try_new(
        BoundedIpc {
            bytes: Vec::new(),
            limit: budget.max_chunk_bytes as usize,
        },
        &batch.schema(),
    )
    .map_err(|_| SourceError::budget("document_ipc_bytes"))?;
    writer
        .write(batch)
        .map_err(|_| SourceError::budget("document_ipc_bytes"))?;
    writer
        .finish()
        .map_err(|_| SourceError::budget("document_ipc_bytes"))?;
    let bytes = writer
        .into_inner()
        .map_err(|_| SourceError::protocol("document_ipc"))?
        .bytes;
    Ok((bytes.len() as u64, format!("{:x}", Sha256::digest(bytes))))
}

// Arrow buffer capacity includes spare allocation, not serialized chunk bytes.
// Enforce the byte budget at the writer instead of rejecting valid wire sizes.
struct BoundedIpc {
    bytes: Vec<u8>,
    limit: usize,
}

impl std::io::Write for BoundedIpc {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::ErrorKind::OutOfMemory.into());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Sorting fixed-size row hashes makes the digest independent of fetch order,
/// without XOR's duplicate cancellation. A range is still bounded by its budget.
pub(crate) fn digest(rows: &[(String, Value)]) -> String {
    let mut hashes: Vec<_> = rows
        .iter()
        .map(|row| Sha256::digest(serde_json::to_vec(row).expect("JSON tuple")).to_vec())
        .collect();
    hashes.sort_unstable();
    let mut hash = Sha256::new();
    for row in hashes {
        hash.update(row);
    }
    format!("{:x}", hash.finalize())
}

#[derive(Default, Debug, Serialize)]
pub struct Stats {
    pub requests: u64,
    pub bytes_received: u64,
    pub busy_us: u64,
    pub rest_us: u64,
}

impl Stats {
    pub(crate) async fn pace(&mut self, started: std::time::Instant, budget: &ReadBudget) {
        let busy = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        let rest = busy.saturating_mul(u64::from(100 - budget.duty_cycle_percent))
            / u64::from(budget.duty_cycle_percent);
        self.requests += 1;
        self.busy_us = self.busy_us.saturating_add(busy);
        tokio::time::sleep(Duration::from_micros(rest)).await;
        self.rest_us = self.rest_us.saturating_add(rest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::Array;
    use serde_json::json;

    #[test]
    fn scalars_and_envelope_preserve_missing_null_and_exact_integer() {
        let cols = vec![ScalarColumn {
            name: "n".into(),
            kind: ScalarType::Int64,
        }];
        let rows = vec![
            (
                "1".into(),
                json!({"n":{"$numberLong":"9223372036854775807"}}),
            ),
            ("2".into(), json!({"n":null})),
            ("3".into(), json!({})),
        ];
        let b = batch(&cols, &rows).unwrap();
        assert_eq!(
            b.column(1)
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap()
                .value(0),
            i64::MAX
        );
        assert!(b.column(1).is_null(1) && b.column(1).is_null(2));
        assert_eq!(b.schema().field(2).metadata()[DOCUMENT_METADATA], "jsonb");
        assert_eq!(batch(&cols, &[]).unwrap().schema(), b.schema());
        assert!(batch(&cols, &[("4".into(), json!({"n":"1"}))]).is_err());
    }

    #[test]
    fn digest_is_order_independent_but_duplicate_sensitive() {
        let a = ("a".into(), serde_json::json!({"x": 1}));
        let b = ("b".into(), serde_json::json!({"x": 2}));
        assert_eq!(digest(&[a.clone(), b.clone()]), digest(&[b, a.clone()]));
        assert_ne!(digest(&[]), digest(&[a.clone(), a]));
    }

    #[test]
    fn budgets_and_reserved_names_fail_closed() {
        assert!(
            validate_columns(&[ScalarColumn {
                name: "_id".into(),
                kind: ScalarType::Text
            }])
            .is_err()
        );
        let b = batch(&[], &[("a".into(), serde_json::json!({}))]).unwrap();
        assert!(
            ipc(
                &b,
                &ReadBudget {
                    max_chunk_bytes: 1,
                    ..ReadBudget::default()
                }
            )
            .is_err()
        );
        assert!(
            validate_budget(&ReadBudget {
                duty_cycle_percent: 0,
                ..ReadBudget::default()
            })
            .is_err()
        );
    }

    #[test]
    fn float_projection_refuses_inexact_integer_coercion() {
        let columns = [ScalarColumn {
            name: "n".into(),
            kind: ScalarType::Float64,
        }];
        assert!(batch(&columns, &[("1".into(), json!({"n":9007199254740993_i64}))]).is_err());
        assert!(batch(&columns, &[("1".into(), json!({"n":1.25}))]).is_ok());
    }
}
