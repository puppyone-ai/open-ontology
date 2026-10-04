//! Pull-one-send-one binary COPY on the caller's connection.
//!
//! The encoder decides the bytes. This module does not open a pool, does not
//! collect a dataset, and does not write a receipt.

use sha2::{Digest, Sha256};
use sqlx::postgres::PgConnection;

use crate::copy_encoder::{
    CopyBudgets, CopyColumn, CopyEncoder, CopyError, CopyPiece, ExactDecimal, TypedBatch,
    UnavailablePolicy,
};

/// Encoder failure, or a driver failure after the encoder accepted the batch.
#[derive(Debug)]
pub enum SinkError {
    /// The public encoder refused the batch. COPY was not finished.
    Encode(CopyError),
    /// The driver rejected the statement or a chunk.
    Driver(&'static str),
}

impl std::fmt::Display for SinkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(error) => write!(formatter, "{error}"),
            Self::Driver(detail) => write!(formatter, "copy sink {detail}"),
        }
    }
}

impl std::error::Error for SinkError {}

/// Copy bytes hashed, and how many row chunks the encoder produced.
#[derive(Debug)]
pub struct HashedCopy {
    /// SHA-256 state after every header, row chunk, and trailer byte.
    pub hasher: Sha256,
    /// Row chunks. A header and a trailer are not included.
    pub row_chunks: u32,
    /// Tuples the encoder emitted.
    pub rows: u64,
}

/// Hash one batch without sending it and without retaining the pieces.
pub fn hash_batch(
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
    budgets: CopyBudgets,
) -> Result<HashedCopy, SinkError> {
    let mut encoder = CopyEncoder::open(
        batch,
        columns,
        positions,
        budgets,
        UnavailablePolicy::Refuse,
    )
    .map_err(SinkError::Encode)?;
    let mut hasher = Sha256::new();
    let mut row_chunks = 0u32;
    let mut rows = 0u64;
    loop {
        match encoder.pull().map_err(SinkError::Encode)? {
            CopyPiece::Done => break,
            CopyPiece::Header { bytes } | CopyPiece::Trailer { bytes } => {
                hasher.update(&bytes);
            }
            CopyPiece::Rows { bytes, row_count } => {
                hasher.update(&bytes);
                row_chunks = row_chunks.saturating_add(1);
                rows = rows.saturating_add(u64::from(row_count));
            }
        }
    }
    Ok(HashedCopy {
        hasher,
        row_chunks,
        rows,
    })
}

/// Send one batch. The first encoder pull happens before COPY starts.
/// A later pull failure aborts COPY.
pub async fn send_batch(
    conn: &mut PgConnection,
    statement: &str,
    batch: &TypedBatch,
    columns: &[CopyColumn],
    positions: &[ExactDecimal],
    budgets: CopyBudgets,
) -> Result<HashedCopy, SinkError> {
    let mut encoder = CopyEncoder::open(
        batch,
        columns,
        positions,
        budgets,
        UnavailablePolicy::Refuse,
    )
    .map_err(SinkError::Encode)?;
    let first = encoder.pull().map_err(SinkError::Encode)?;
    let mut copy = conn
        .copy_in_raw(statement)
        .await
        .map_err(|_| SinkError::Driver("copy_start"))?;
    let mut hasher = Sha256::new();
    let mut row_chunks = 0u32;
    let mut rows = 0u64;
    absorb(&mut hasher, &mut row_chunks, &mut rows, &first);
    if copy.send(piece_bytes(&first)).await.is_err() {
        let _ = copy.abort("copy send failed").await;
        return Err(SinkError::Driver("copy_send"));
    }
    loop {
        let piece = match encoder.pull() {
            Ok(piece) => piece,
            Err(error) => {
                let _ = copy.abort(error.code).await;
                return Err(SinkError::Encode(error));
            }
        };
        if matches!(piece, CopyPiece::Done) {
            break;
        }
        absorb(&mut hasher, &mut row_chunks, &mut rows, &piece);
        if copy.send(piece_bytes(&piece)).await.is_err() {
            let _ = copy.abort("copy send failed").await;
            return Err(SinkError::Driver("copy_send"));
        }
    }
    copy.finish()
        .await
        .map_err(|_| SinkError::Driver("copy_finish"))?;
    Ok(HashedCopy {
        hasher,
        row_chunks,
        rows,
    })
}

fn absorb(hasher: &mut Sha256, row_chunks: &mut u32, rows: &mut u64, piece: &CopyPiece) {
    match piece {
        CopyPiece::Done => {}
        CopyPiece::Header { bytes } | CopyPiece::Trailer { bytes } => {
            hasher.update(bytes);
        }
        CopyPiece::Rows { bytes, row_count } => {
            hasher.update(bytes);
            *row_chunks = row_chunks.saturating_add(1);
            *rows = rows.saturating_add(u64::from(*row_count));
        }
    }
}

fn piece_bytes(piece: &CopyPiece) -> &[u8] {
    match piece {
        CopyPiece::Header { bytes } | CopyPiece::Rows { bytes, .. } | CopyPiece::Trailer { bytes } => {
            bytes
        }
        CopyPiece::Done => &[],
    }
}
