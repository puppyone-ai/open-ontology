//! Bridge a [`SourceEngine`](oo_connectors::engine::SourceEngine) receipt and its
//! sealed canonical Arrow IPC into the existing typed loader representation.
//!
//! Call once per chunk, after the engine's sink has durably closed the IPC file.
//! The returned [`DecodedStream`] feeds `native_c5_parts_stage::stage` without
//! another type conversion. Admission, lease fencing, range completeness and
//! publication remain the existing loader's responsibility: a chunk receipt is
//! a content witness, NOT permission to write a target.
//!
//! No key-bound comparison or translation occurs here. Engine plans use
//! `[lo, hi)` while legacy C5 topology has its own source-authenticated bounds.
//! In particular this adapter does not relabel a PostgreSQL receipt as MySQL to
//! pass the current MySQL-only multipart admission contract.
//!
//! Receipt hashes cover the original IPC, including field metadata. Only after
//! validation does the adapter apply Oracle's per-field empty-string flag.
//! Unflagged text and JSON content are left unchanged.

use oo_connectors::engine::{ChunkReceipt, ChunkSpec, ReadBudget};
use sha2::{Digest, Sha256};

use crate::native_arrow::{DecodedStream, NativeCell, arrow_field_metadata, decode_arrow_stream};

/// Verify identity, budget, IPC digest and row count before exposing typed rows.
/// Watermarks are retained by the caller with the original receipt; they are
/// not synthesized into a consistency or publication claim.
pub fn decode_chunk(
    chunk: &ChunkSpec,
    receipt: &ChunkReceipt,
    ipc: &[u8],
    budget: &ReadBudget,
) -> Result<DecodedStream, &'static str> {
    if receipt.table != chunk.table
        || receipt.plan_revision != chunk.plan_revision
        || receipt.chunk_id != chunk.chunk_id
    {
        return Err("engine_chunk_identity");
    }
    // Never allocate from the receipt's counts or accept a caller-expanded cap.
    let max_rows = budget.max_chunk_rows.min(100_000);
    let max_bytes = budget.max_chunk_bytes.min(32 * 1024 * 1024);
    if max_rows == 0
        || max_bytes == 0
        || receipt.rows > max_rows
        || receipt.bytes > max_bytes
        || ipc.len() as u64 > max_bytes
    {
        return Err("engine_chunk_budget");
    }
    if receipt.bytes != ipc.len() as u64 || receipt.sha256 != format!("{:x}", Sha256::digest(ipc)) {
        return Err("engine_chunk_digest");
    }
    let mut decoded = decode_arrow_stream(ipc).map_err(|_| "engine_chunk_arrow")?;
    if decoded.rows as u64 != receipt.rows {
        return Err("engine_chunk_rows");
    }
    let metadata = arrow_field_metadata(ipc).map_err(|_| "engine_chunk_metadata")?;
    apply_empty_string_semantics(&mut decoded, &metadata)?;
    Ok(decoded)
}

fn apply_empty_string_semantics(
    decoded: &mut DecodedStream,
    metadata: &[std::collections::BTreeMap<String, String>],
) -> Result<(), &'static str> {
    if metadata.len() != decoded.fields.len() {
        return Err("engine_chunk_metadata");
    }
    for ((field, cells), metadata) in decoded
        .fields
        .iter()
        .zip(&mut decoded.columns)
        .zip(metadata)
    {
        match metadata
            .get("oo.oracle.empty_string_is_null")
            .map(String::as_str)
        {
            None | Some("false") => {}
            Some("true") if field.arrow_type == "utf8" && field.nullable => {
                for cell in cells {
                    if matches!(cell, NativeCell::Utf8(text) if text.is_empty()) {
                        *cell = NativeCell::Null;
                    }
                }
            }
            _ => return Err("engine_chunk_empty_string_semantics"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_arrow::DecodedField;
    use oo_connectors::engine::{TableRef, Watermark};

    #[test]
    fn receipt_hash_covers_original_bytes_before_flagged_cells_are_normalized() {
        let producer = FixtureProducer::new();
        let ipc = include_bytes!("engine_chunk/empty-semantics.arrow");
        let mut receipt = producer.receipt();
        receipt.rows = 4;
        receipt.bytes = ipc.len() as u64;
        receipt.sha256 = format!("{:x}", Sha256::digest(ipc));
        let raw = decode_arrow_stream(ipc).unwrap();
        let decoded = decode_chunk(&producer.chunk, &receipt, ipc, &ReadBudget::default()).unwrap();
        assert_eq!(raw.columns[0][0], NativeCell::Utf8(String::new()));
        assert_eq!(decoded.columns[0][0], NativeCell::Null);
        assert_eq!(decoded.columns[1], raw.columns[1]);
        assert_eq!(&decoded.columns[0][1..], &raw.columns[0][1..]);
        assert_eq!(decoded.rows, 4);
        assert_eq!(
            arrow_field_metadata(ipc).unwrap()[0]["oo.oracle.type"],
            "VARCHAR2"
        );
        let mut damaged = ipc.to_vec();
        let flag = damaged.windows(4).position(|b| b == b"true").unwrap();
        damaged[flag] = b'f';
        assert_eq!(
            decode_chunk(&producer.chunk, &receipt, &damaged, &ReadBudget::default()).unwrap_err(),
            "engine_chunk_digest"
        );
    }

    #[test]
    fn oracle_empty_semantics_are_per_column_and_do_not_parse_text_as_json() {
        let field = DecodedField {
            name: "text".into(),
            nullable: true,
            arrow_type: "utf8".into(),
            precision: None,
            scale: None,
            timezone: None,
        };
        let cells = vec![
            NativeCell::Utf8(String::new()),
            NativeCell::Null,
            NativeCell::Utf8(" ".into()),
            NativeCell::Utf8(r#"{"value":""}"#.into()),
        ];
        let mut decoded = DecodedStream {
            rows: cells.len(),
            fields: vec![field.clone(), field],
            columns: vec![cells.clone(), cells.clone()],
        };
        let flagged = std::collections::BTreeMap::from([(
            "oo.oracle.empty_string_is_null".into(),
            "true".into(),
        )]);
        apply_empty_string_semantics(&mut decoded, &[flagged.clone(), Default::default()]).unwrap();
        assert_eq!(decoded.columns[0][0], NativeCell::Null);
        assert_eq!(&decoded.columns[0][1..], &cells[1..]);
        assert_eq!(decoded.columns[1], cells);
        let disabled = std::collections::BTreeMap::from([(
            "oo.oracle.empty_string_is_null".into(),
            "false".into(),
        )]);
        decoded.columns[0] = cells.clone();
        apply_empty_string_semantics(&mut decoded, &[disabled, Default::default()]).unwrap();
        assert_eq!(decoded.columns[0], cells);

        for value in ["TRUE", "yes", ""] {
            let mut invalid = flagged.clone();
            invalid.insert("oo.oracle.empty_string_is_null".into(), value.into());
            assert_eq!(
                apply_empty_string_semantics(&mut decoded, &[invalid, Default::default()]),
                Err("engine_chunk_empty_string_semantics")
            );
        }
        decoded.fields[0].nullable = false;
        assert_eq!(
            apply_empty_string_semantics(&mut decoded, &[flagged.clone(), Default::default()]),
            Err("engine_chunk_empty_string_semantics")
        );
        decoded.fields[0].nullable = true;
        decoded.fields[0].arrow_type = "binary".into();
        assert_eq!(
            apply_empty_string_semantics(&mut decoded, &[flagged, Default::default()]),
            Err("engine_chunk_empty_string_semantics")
        );
    }

    // This fixture is not a SourceEngine implementation or a target-load test.
    struct FixtureProducer {
        chunk: ChunkSpec,
        ipc: &'static [u8],
    }

    impl FixtureProducer {
        fn new() -> Self {
            Self {
                chunk: ChunkSpec {
                    table: TableRef {
                        schema: "fixture".into(),
                        name: "items".into(),
                    },
                    plan_revision: 7,
                    chunk_id: 2,
                    lo: None,
                    hi: None,
                    estimated_rows: None,
                },
                ipc: include_bytes!("../tests/fixtures/c1_parts/fill-00000001.arrow"),
            }
        }

        fn receipt(&self) -> ChunkReceipt {
            ChunkReceipt {
                table: self.chunk.table.clone(),
                plan_revision: self.chunk.plan_revision,
                chunk_id: self.chunk.chunk_id,
                rows: 3,
                bytes: 928,
                sha256: "d6a2f5671490b7e52a22f08cd46bf7e406c7e914e8f89848af9eaa577c39cbc2".into(),
                low: Some(Watermark::PgLsn { lsn: 10 }),
                high: Some(Watermark::PgLsn { lsn: 20 }),
            }
        }
    }

    #[test]
    fn fixture_receipt_decodes_to_existing_typed_representation() {
        let engine = FixtureProducer::new();
        let receipt = engine.receipt();
        let decoded =
            decode_chunk(&engine.chunk, &receipt, engine.ipc, &ReadBudget::default()).unwrap();
        assert_eq!(decoded, decode_arrow_stream(engine.ipc).unwrap());
        assert_eq!(decoded.rows, 3);
        assert_eq!(receipt.low, Some(Watermark::PgLsn { lsn: 10 }));
    }

    #[test]
    fn empty_chunk_keeps_its_schema_but_malformed_ipc_is_not_a_chunk() {
        let engine = FixtureProducer::new();
        let mut receipt = engine.receipt();
        receipt.rows = 0;
        receipt.bytes = 736;
        receipt.sha256 = "3ffc5ba0f97269feaf6321869118836a622ad8c8f8086005728ce67b16725a93".into();
        let empty = include_bytes!("../tests/fixtures/c1_parts/fill-00000003.arrow");
        let decoded = decode_chunk(&engine.chunk, &receipt, empty, &ReadBudget::default()).unwrap();
        assert_eq!(decoded.rows, 0);
        assert!(!decoded.fields.is_empty());
        receipt.bytes = 3;
        receipt.sha256 = format!("{:x}", Sha256::digest(b"bad"));
        assert_eq!(
            decode_chunk(&engine.chunk, &receipt, b"bad", &ReadBudget::default()).unwrap_err(),
            "engine_chunk_arrow"
        );
    }

    #[test]
    fn swapped_stale_corrupt_and_oversized_chunks_are_refused() {
        let engine = FixtureProducer::new();
        let good = engine.receipt();
        let check = |receipt: &ChunkReceipt, ipc: &[u8], budget: &ReadBudget| {
            decode_chunk(&engine.chunk, receipt, ipc, budget).unwrap_err()
        };
        let budget = ReadBudget::default();
        for field in 0..3 {
            let mut bad = good.clone();
            match field {
                0 => bad.table.name = "other".into(),
                1 => bad.plan_revision += 1,
                _ => bad.chunk_id += 1,
            }
            assert_eq!(check(&bad, engine.ipc, &budget), "engine_chunk_identity");
        }
        let mut bad = good.clone();
        bad.rows += 1;
        assert_eq!(check(&bad, engine.ipc, &budget), "engine_chunk_rows");
        let mut bad = good.clone();
        bad.bytes += 1;
        assert_eq!(check(&bad, engine.ipc, &budget), "engine_chunk_digest");
        let mut corrupted = engine.ipc.to_vec();
        corrupted[0] ^= 1;
        assert_eq!(check(&good, &corrupted, &budget), "engine_chunk_digest");
        assert_eq!(
            check(
                &good,
                engine.ipc,
                &ReadBudget {
                    max_chunk_bytes: 1,
                    ..budget
                }
            ),
            "engine_chunk_budget"
        );
        let mut bad = good;
        bad.rows = 100_001;
        assert_eq!(
            check(
                &bad,
                engine.ipc,
                &ReadBudget {
                    max_chunk_rows: u64::MAX,
                    ..budget
                }
            ),
            "engine_chunk_budget"
        );
    }
}
