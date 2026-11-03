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

use oo_connectors::engine::{ChunkReceipt, ChunkSpec, ReadBudget};
use sha2::{Digest, Sha256};

use crate::native_arrow::{DecodedStream, decode_arrow_stream};

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
    let decoded = decode_arrow_stream(ipc).map_err(|_| "engine_chunk_arrow")?;
    if decoded.rows as u64 != receipt.rows {
        return Err("engine_chunk_rows");
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_connectors::engine::{TableRef, Watermark};

    // A fake producer emits the same receipt + sealed IPC pair as an engine sink.
    struct FakeEngine {
        chunk: ChunkSpec,
        ipc: &'static [u8],
    }

    impl FakeEngine {
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
    fn fake_engine_receipt_feeds_existing_typed_loader() {
        let engine = FakeEngine::new();
        let receipt = engine.receipt();
        let decoded =
            decode_chunk(&engine.chunk, &receipt, engine.ipc, &ReadBudget::default()).unwrap();
        assert_eq!(decoded, decode_arrow_stream(engine.ipc).unwrap());
        assert_eq!(decoded.rows, 3);
        assert_eq!(receipt.low, Some(Watermark::PgLsn { lsn: 10 }));
    }

    #[test]
    fn empty_chunk_keeps_its_schema_but_malformed_ipc_is_not_a_chunk() {
        let engine = FakeEngine::new();
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
        let engine = FakeEngine::new();
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
