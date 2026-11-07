//! Loader side of one `chunked-typed/v2` C1 part.
//!
//! A snapshot chunk or a CDC batch part is read through [`crate::c1_parts`]
//! (size, SHA-256, row count and schema are checked there). This module checks
//! the column names and the key range against the request, renders every typed
//! cell as PostgreSQL COPY text for the stage column type read from the catalog,
//! and streams the rows into the per-table stage. COPY writes the stage tags
//! (`_oo_part_kind`, `_oo_part_no`, `_oo_op`, `_oo_pos`) itself, so the runtime
//! role needs only the INSERT grant that migration 0042 gives it.
//!
//! The caller owns the transaction. Every write transaction takes the task lock
//! first (DESIGN R2). Nothing here opens a connection or touches the fence.
//! Design: `specs/chunked-baseline/DESIGN.md` §3.1, §3.4, §4.1.

use std::fmt::Write as _;

use sqlx::{PgConnection, Postgres, Transaction};

use crate::c1_parts::{self, PartEntry, PartKind};
use crate::native_arrow::{DecodedStream, NativeCell};

/// Stage tag columns, in stage order, before the physical value columns.
pub(crate) const TAG_COLUMNS: [&str; 4] = ["_oo_part_kind", "_oo_part_no", "_oo_op", "_oo_pos"];
const COPY_FLUSH_BYTES: usize = 256 * 1024;

/// One physical value column of a stage, as the catalog reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StageColumn {
    pub(crate) name: String,
    pub(crate) oid: i64,
    pub(crate) typmod: i32,
}

/// Names derived from `(source, task, schema, table)` as migration 0042 derives
/// them, and the stage columns read from the catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StageTarget {
    pub(crate) stage: String,
    pub(crate) columns: Vec<StageColumn>,
}

/// Map a database error to a closed token. A `OO030` raise carries the SQL
/// token in its message. Unknown messages do not leak.
pub(crate) fn sql_error(err: sqlx::Error) -> &'static str {
    const TOKENS: &[&str] = &[
        "chunk_conflict",
        "chunk_sequence",
        "chunk_part_rows",
        "chunk_keys",
        "invalid_chunk",
        "batch_conflict",
        "batch_sequence",
        "batch_rows",
        "invalid_batch",
        "invalid_batch_events",
        "table_identity_conflict",
        "closed_baseline_table",
        "closed_baseline_key",
        "format_mismatch",
        "seal_conflict",
        "seal_tables",
        "composed_row_count",
        "snapshot_incomplete",
        "table_unregistered",
    ];
    let Some(db) = err.as_database_error() else {
        eprintln!("oo-pipeline: c1 v2 driver error {err}");
        return "target_apply";
    };
    let state = db.code().map(|code| code.to_string()).unwrap_or_default();
    let message = db.message();
    // Our own raises carry fixed texts. Other messages can echo source values, so they stay out of logs.
    if state.starts_with("OO") {
        eprintln!("oo-pipeline: c1 v2 sqlstate={state} {message}");
    } else {
        eprintln!("oo-pipeline: c1 v2 sqlstate={state}");
    }
    match state.as_str() {
        "OO010" => "lease_or_barrier",
        "OO024" => "physical_mapping_absent",
        "OO031" => "run_not_open",
        "OO030" => TOKENS
            .iter()
            .copied()
            .find(|token| *token == message)
            .unwrap_or("closed"),
        "42501" => "permission",
        "42P01" => "not_ready",
        "55P03" => "fence_held",
        "22P02" | "22023" => "closed",
        "22003" => "read_budget",
        _ => "target_apply",
    }
}

/// Read the stage name, tomb name and stage columns of one table of a v2 task.
pub(crate) async fn stage_target(
    tx: &mut Transaction<'_, Postgres>,
    source: &str,
    task: &str,
    schema: &str,
    table: &str,
) -> Result<StageTarget, &'static str> {
    let (stage, _tomb): (String, String) = sqlx::query_as(
        "SELECT 'cs_' || md5($1 || ':' || $2 || ':' || $3 || '.' || $4), \
                'c1t_' || left(md5($1 || ':' || $2 || ':' || $3 || '.' || $4), 24)",
    )
    .bind(source)
    .bind(task)
    .bind(schema)
    .bind(table)
    .fetch_one(&mut **tx)
    .await
    .map_err(sql_error)?;
    let rows: Vec<(String, i64, i32)> = sqlx::query_as(
        "SELECT a.attname::text, a.atttypid::int8, a.atttypmod::int4 \
         FROM pg_attribute a \
         WHERE a.attrelid = to_regclass($1) AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
    .bind(format!("_oo.{stage}"))
    .fetch_all(&mut **tx)
    .await
    .map_err(sql_error)?;
    if rows.len() <= TAG_COLUMNS.len()
        || rows
            .iter()
            .take(TAG_COLUMNS.len())
            .map(|row| row.0.as_str())
            .ne(TAG_COLUMNS.iter().copied())
    {
        return Err("stage_absent");
    }
    let columns = rows
        .into_iter()
        .skip(TAG_COLUMNS.len())
        .map(|(name, oid, typmod)| StageColumn { name, oid, typmod })
        .collect();
    Ok(StageTarget { stage, columns })
}

/// Which tag set a COPY row carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tags {
    /// Snapshot chunk `chunk_no`. `_oo_op` and `_oo_pos` are NULL.
    Snapshot { part_no: i32 },
    /// CDC batch `batch_no`. `_oo_op` and `_oo_pos` come from the part.
    Cdc { part_no: i32 },
}

/// Streams a validated part as COPY text, one bounded piece at a time.
pub(crate) struct CopyRows<'a> {
    decoded: &'a DecodedStream,
    tags: Tags,
    columns: &'a [StageColumn],
    next: usize,
}

impl<'a> CopyRows<'a> {
    /// Check the field names against `expected` and the field count against the tags.
    pub(crate) fn new(
        decoded: &'a DecodedStream,
        expected: &[String],
        target: &'a [StageColumn],
        tags: Tags,
    ) -> Result<Self, &'static str> {
        let extra = match tags {
            Tags::Snapshot { .. } => 0,
            Tags::Cdc { .. } => 2,
        };
        if expected.len() != target.len() || decoded.fields.len() != expected.len() + extra {
            return Err("part_schema");
        }
        if decoded
            .fields
            .iter()
            .zip(expected)
            .any(|(field, name)| &field.name != name)
        {
            return Err("part_schema");
        }
        Ok(Self {
            decoded,
            tags,
            columns: target,
            next: 0,
        })
    }

    /// Next piece of COPY text, at least one row. `None` once every row is written.
    pub(crate) fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, &'static str> {
        if self.next >= self.decoded.rows {
            return Ok(None);
        }
        let mut out = Vec::with_capacity(COPY_FLUSH_BYTES + 4096);
        while self.next < self.decoded.rows && out.len() < COPY_FLUSH_BYTES {
            self.write_row(&mut out)?;
            self.next += 1;
        }
        Ok(Some(out))
    }

    fn write_row(&self, out: &mut Vec<u8>) -> Result<(), &'static str> {
        let row = self.next;
        let user = self.columns.len();
        match self.tags {
            Tags::Snapshot { part_no } => {
                let _ = write!(OutWriter(out), "snapshot\t{part_no}\t\\N\t\\N");
            }
            Tags::Cdc { part_no } => {
                let op = match &self.decoded.columns[user + 1][row] {
                    NativeCell::Utf8(text)
                        if text == c1_parts::OP_UPSERT || text == c1_parts::OP_DELETE =>
                    {
                        text.as_str()
                    }
                    _ => return Err("cdc_op"),
                };
                let pos = match &self.decoded.columns[user][row] {
                    NativeCell::Decimal {
                        coefficient,
                        scale: 0,
                        ..
                    } if *coefficient >= 0 => coefficient.to_string(),
                    _ => return Err("cdc_pos"),
                };
                let _ = write!(OutWriter(out), "cdc\t{part_no}\t{op}\t{pos}");
            }
        }
        for (index, column) in self.columns.iter().enumerate() {
            out.push(b'\t');
            cell_text(
                out,
                column.oid,
                column.typmod,
                &self.decoded.columns[index][row],
            )?;
        }
        out.push(b'\n');
        Ok(())
    }
}

/// `fmt::Write` over a byte vector, so small numbers are written without a temporary.
struct OutWriter<'a>(&'a mut Vec<u8>);

impl std::fmt::Write for OutWriter<'_> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0.extend_from_slice(text.as_bytes());
        Ok(())
    }
}

/// Render one cell as COPY text for a column of type `oid`. The cell variant must
/// match the target type. A mismatch is refused, never converted.
pub(crate) fn cell_text(
    out: &mut Vec<u8>,
    oid: i64,
    typmod: i32,
    cell: &NativeCell,
) -> Result<(), &'static str> {
    match (oid, cell) {
        (_, NativeCell::Null) => out.extend_from_slice(b"\\N"),
        (16, NativeCell::Bool(value)) => out.push(if *value { b't' } else { b'f' }),
        (20 | 21 | 23, NativeCell::Int { value, .. }) => int_text(out, oid, i128::from(*value))?,
        (20 | 21 | 23, NativeCell::UInt8(value)) => int_text(out, oid, i128::from(*value))?,
        (20 | 21 | 23, NativeCell::UInt64(value)) => int_text(out, oid, i128::from(*value))?,
        (700 | 701, NativeCell::Float64(bits)) => float_text(out, f64::from_bits(*bits)),
        (
            1700,
            NativeCell::Decimal {
                scale, coefficient, ..
            },
        ) => numeric_text(out, typmod, *scale, *coefficient)?,
        (25 | 1042 | 1043 | 1083 | 1186 | 2950, NativeCell::Utf8(text)) => {
            escape(out, text.as_bytes())
        }
        (114 | 3802, NativeCell::Json(text) | NativeCell::Utf8(text)) => {
            escape(out, text.as_bytes())
        }
        (17, NativeCell::Binary(bytes)) => {
            // COPY text unescapes one backslash, so the bytea input receives `\x...`.
            out.extend_from_slice(b"\\\\x");
            for byte in bytes {
                let _ = write!(OutWriter(out), "{byte:02x}");
            }
        }
        (1082, NativeCell::Date32(days)) => date_text(out, i64::from(*days))?,
        (1114, NativeCell::TimestampMicros(micros)) => timestamp_text(out, *micros, false)?,
        (1184, NativeCell::TimestampMicrosUtc { value, timezone }) if timezone == "UTC" => {
            timestamp_text(out, *value, true)?;
        }
        (
            1186,
            NativeCell::Int {
                bit_width: 64,
                value,
            },
        ) => {
            let _ = write!(OutWriter(out), "{value} microseconds");
        }
        _ => return Err("cell_type"),
    }
    Ok(())
}

fn int_text(out: &mut Vec<u8>, oid: i64, value: i128) -> Result<(), &'static str> {
    let fits = match oid {
        21 => i16::try_from(value).is_ok(),
        23 => i32::try_from(value).is_ok(),
        _ => i64::try_from(value).is_ok(),
    };
    if !fits {
        return Err("int_range");
    }
    let _ = write!(OutWriter(out), "{value}");
    Ok(())
}

fn float_text(out: &mut Vec<u8>, value: f64) {
    let text = if value.is_nan() {
        "NaN".to_owned()
    } else if value == f64::INFINITY {
        "Infinity".to_owned()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        format!("{value:?}")
    };
    out.extend_from_slice(text.as_bytes());
}

fn numeric_text(
    out: &mut Vec<u8>,
    typmod: i32,
    scale: i8,
    coefficient: i128,
) -> Result<(), &'static str> {
    if scale < 0 {
        // Negative-scale sources load into numeric(p+s,0): only scale 0 typmods fit.
        if typmod >= 4 && (typmod - 4) & 0xffff != 0 {
            return Err("precision");
        }
        out.extend_from_slice(coefficient.to_string().as_bytes());
        if coefficient != 0 {
            out.extend(std::iter::repeat_n(b'0', (-i16::from(scale)) as usize));
        }
        return Ok(());
    }
    let scale = usize::try_from(scale).map_err(|_| "precision")?;
    if typmod >= 4 && usize::try_from((typmod - 4) & 0xffff).ok() != Some(scale) {
        return Err("precision");
    }
    let digits = coefficient.unsigned_abs().to_string();
    if coefficient < 0 {
        out.push(b'-');
    }
    if scale == 0 {
        out.extend_from_slice(digits.as_bytes());
        return Ok(());
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let split = padded.len() - scale;
    out.extend_from_slice(&padded.as_bytes()[..split]);
    out.push(b'.');
    out.extend_from_slice(&padded.as_bytes()[split..]);
    Ok(())
}

fn escape(out: &mut Vec<u8>, bytes: &[u8]) {
    for byte in bytes {
        match byte {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            other => out.push(*other),
        }
    }
}

fn date_text(out: &mut Vec<u8>, days: i64) -> Result<(), &'static str> {
    let (year, month, day) = civil_from_days(days);
    if !(1..=9999).contains(&year) {
        return Err("date_range");
    }
    let _ = write!(OutWriter(out), "{year:04}-{month:02}-{day:02}");
    Ok(())
}

fn timestamp_text(out: &mut Vec<u8>, micros: i64, utc: bool) -> Result<(), &'static str> {
    const DAY: i64 = 86_400_000_000;
    let days = micros.div_euclid(DAY);
    let rem = micros.rem_euclid(DAY);
    let (year, month, day) = civil_from_days(days);
    if !(1..=9999).contains(&year) {
        return Err("date_range");
    }
    let hours = rem / 3_600_000_000;
    let minutes = (rem / 60_000_000) % 60;
    let seconds = (rem / 1_000_000) % 60;
    let fraction = rem % 1_000_000;
    let _ = write!(
        OutWriter(out),
        "{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}.{fraction:06}"
    );
    if utc {
        out.extend_from_slice(b"+00");
    }
    Ok(())
}

/// Civil date of a day count from 1970-01-01. Proleptic Gregorian.
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

/// Key of one row. Integer key cells are accepted. A `BIGINT UNSIGNED` key arrives as
/// a scale-0 decimal (`numeric(20,0)`, see `map.rs`), so such a cell is accepted when
/// its value is inside the key domain. Anything else is refused.
pub(crate) fn key_value(cell: &NativeCell) -> Result<i128, &'static str> {
    match cell {
        NativeCell::Int { value, .. } => Ok(i128::from(*value)),
        NativeCell::UInt8(value) => Ok(i128::from(*value)),
        NativeCell::UInt64(value) => Ok(i128::from(*value)),
        NativeCell::Decimal {
            coefficient,
            scale: 0,
            ..
        } => {
            if (i128::from(i64::MIN)..=i128::from(u64::MAX)).contains(coefficient) {
                Ok(*coefficient)
            } else {
                Err("key_range")
            }
        }
        _ => Err("key_type"),
    }
}

/// Min and max key of a decoded part must equal the entry's bounds. An empty part has none.
pub(crate) fn check_keys(
    decoded: &DecodedStream,
    key_index: usize,
    entry: &PartEntry,
) -> Result<(), &'static str> {
    if decoded.rows == 0 {
        return if entry.key_lo.is_none() && entry.key_hi.is_none() {
            Ok(())
        } else {
            Err("part_keys")
        };
    }
    let mut low = i128::MAX;
    let mut high = i128::MIN;
    for cell in &decoded.columns[key_index] {
        let key = key_value(cell)?;
        low = low.min(key);
        high = high.max(key);
    }
    if entry.key_lo != Some(low) || entry.key_hi != Some(high) {
        return Err("part_keys");
    }
    Ok(())
}

/// Read and verify one part file. Size, SHA-256, rows and schema are checked by
/// `c1_parts::read_part`. The file name must be the canonical name for `kind` and `part_no`.
pub(crate) fn read_verified(
    path: &std::path::Path,
    kind: PartKind,
    entry: &PartEntry,
) -> Result<DecodedStream, &'static str> {
    let name = c1_parts::part_file_name(kind, entry.part_no).map_err(|fault| fault.reason)?;
    if path.file_name().and_then(|file| file.to_str()) != Some(name.as_str()) {
        return Err("part_path");
    }
    let dir = path.parent().ok_or("part_path")?;
    c1_parts::read_part(dir, entry).map_err(|fault| fault.reason)
}

/// Stream every row of `rows` into the stage. Returns the rows COPY accepted.
pub(crate) async fn copy_into_stage(
    tx: &mut Transaction<'_, Postgres>,
    stage: &str,
    columns: &[StageColumn],
    rows: &mut CopyRows<'_>,
) -> Result<u64, &'static str> {
    let mut list: Vec<String> = TAG_COLUMNS.iter().map(|tag| (*tag).to_owned()).collect();
    list.extend(columns.iter().map(|column| quote_ident(&column.name)));
    let statement = format!(
        "COPY _oo.{} ({}) FROM STDIN",
        quote_ident(stage),
        list.join(", ")
    );
    let conn: &mut PgConnection = tx;
    let mut copy = conn.copy_in_raw(&statement).await.map_err(sql_error)?;
    loop {
        match rows.next_chunk() {
            Ok(Some(bytes)) => {
                if copy.send(bytes).await.is_err() {
                    let _ = copy.abort("copy send failed").await;
                    return Err("copy_send");
                }
            }
            Ok(None) => break,
            Err(token) => {
                let _ = copy.abort(token).await;
                return Err(token);
            }
        }
    }
    copy.finish().await.map_err(sql_error)
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Count the delete events of a CDC part. Each distinct key of a batch gives at
/// most one tombstone row, so this bounds the tombstones the batch can add.
pub(crate) fn delete_count(decoded: &DecodedStream, user: usize) -> Result<u64, &'static str> {
    let mut deletes = 0u64;
    for cell in &decoded.columns[user + 1] {
        match cell {
            NativeCell::Utf8(text) if text == c1_parts::OP_DELETE => deletes += 1,
            NativeCell::Utf8(text) if text == c1_parts::OP_UPSERT => {}
            _ => return Err("cdc_op"),
        }
    }
    Ok(deletes)
}

/// Exact row count of one tombstone table (N6). The runtime role cannot read the
/// table, so migration 0043 counts it. The count runs under the task lock, so it
/// is the count the batch sees.
pub(crate) async fn tombstone_count(
    tx: &mut Transaction<'_, Postgres>,
    envelope: &crate::native_c1_baseline::Envelope,
    schema: &str,
    table: &str,
) -> Result<u64, &'static str> {
    let live: i64 =
        sqlx::query_scalar("SELECT _oo.native_c1_baseline_v2_tomb_count($1,$2::uuid,$3,$4,$5,$6)")
            .bind(&envelope.source)
            .bind(&envelope.task)
            .bind(envelope.generation)
            .bind(envelope.epoch)
            .bind(schema)
            .bind(table)
            .fetch_one(&mut **tx)
            .await
            .map_err(sql_error)?;
    u64::try_from(live).map_err(|_| "closed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(oid: i64, typmod: i32, cell: NativeCell) -> String {
        let mut out = Vec::new();
        cell_text(&mut out, oid, typmod, &cell).expect("render");
        String::from_utf8(out).expect("utf8")
    }

    fn refused(oid: i64, typmod: i32, cell: NativeCell) -> &'static str {
        let mut out = Vec::new();
        cell_text(&mut out, oid, typmod, &cell).expect_err("refused")
    }

    #[test]
    fn civil_dates_match_known_day_counts() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(-719_162), (1, 1, 1));
        assert_eq!(civil_from_days(2_932_896), (9999, 12, 31));
    }

    #[test]
    fn decimal_keys_of_unsigned_bigint_round_trip_up_to_u64_max() {
        // F3: `BIGINT UNSIGNED` keys are scale-0 decimals. The maximum must survive.
        let max = i128::from(u64::MAX);
        assert_eq!(
            key_value(&NativeCell::Decimal {
                precision: 20,
                scale: 0,
                coefficient: max
            }),
            Ok(max)
        );
        assert_eq!(
            key_value(&NativeCell::Decimal {
                precision: 20,
                scale: 0,
                coefficient: 0
            }),
            Ok(0)
        );
        assert_eq!(
            key_value(&NativeCell::Decimal {
                precision: 20,
                scale: 0,
                coefficient: i128::from(i64::MIN)
            }),
            Ok(i128::from(i64::MIN))
        );
        assert_eq!(
            key_value(&NativeCell::Decimal {
                precision: 20,
                scale: 0,
                coefficient: max + 1
            }),
            Err("key_range")
        );
        assert_eq!(
            key_value(&NativeCell::Decimal {
                precision: 20,
                scale: 2,
                coefficient: 5
            }),
            Err("key_type")
        );
        assert_eq!(key_value(&NativeCell::Utf8("1".into())), Err("key_type"));
        // The decimal key text round-trips through the chunk frame and the key bounds.
        assert_eq!(max.to_string(), "18446744073709551615");
        assert_eq!("18446744073709551615".parse::<i128>().expect("parse"), max);
    }

    #[test]
    fn integers_are_range_checked_per_target_type() {
        assert_eq!(
            render(
                20,
                -1,
                NativeCell::Int {
                    bit_width: 64,
                    value: i64::MIN
                }
            ),
            "-9223372036854775808"
        );
        assert_eq!(
            render(
                21,
                -1,
                NativeCell::Int {
                    bit_width: 16,
                    value: -32768
                }
            ),
            "-32768"
        );
        assert_eq!(
            render(20, -1, NativeCell::UInt64(i64::MAX as u64)),
            "9223372036854775807"
        );
        assert_eq!(
            refused(
                21,
                -1,
                NativeCell::Int {
                    bit_width: 32,
                    value: 32768
                }
            ),
            "int_range"
        );
        assert_eq!(
            refused(23, -1, NativeCell::UInt64(u64::from(u32::MAX) + 1)),
            "int_range"
        );
        assert_eq!(refused(20, -1, NativeCell::UInt64(u64::MAX)), "int_range");
    }

    #[test]
    fn text_is_escaped_so_copy_keeps_it_one_field() {
        assert_eq!(
            render(25, -1, NativeCell::Utf8("a\tb\nc\\d\re".into())),
            "a\\tb\\nc\\\\d\\re"
        );
        assert_eq!(render(25, -1, NativeCell::Utf8("\\N".into())), "\\\\N");
        assert_eq!(
            render(25, -1, NativeCell::Utf8("héllo 世界".into())),
            "héllo 世界"
        );
    }

    #[test]
    fn null_is_copy_null_and_bool_is_t_or_f() {
        assert_eq!(render(25, -1, NativeCell::Null), "\\N");
        assert_eq!(render(16, -1, NativeCell::Bool(true)), "t");
        assert_eq!(render(16, -1, NativeCell::Bool(false)), "f");
    }

    #[test]
    fn decimals_keep_the_column_scale() {
        // numeric(10,2): typmod = ((10 << 16) | 2) + 4
        let typmod = ((10 << 16) | 2) + 4;
        assert_eq!(
            render(
                1700,
                typmod,
                NativeCell::Decimal {
                    precision: 10,
                    scale: 2,
                    coefficient: 150
                }
            ),
            "1.50"
        );
        assert_eq!(
            render(
                1700,
                typmod,
                NativeCell::Decimal {
                    precision: 10,
                    scale: 2,
                    coefficient: -5
                }
            ),
            "-0.05"
        );
        assert_eq!(
            render(
                1700,
                typmod,
                NativeCell::Decimal {
                    precision: 10,
                    scale: 2,
                    coefficient: 0
                }
            ),
            "0.00"
        );
        assert_eq!(
            refused(
                1700,
                typmod,
                NativeCell::Decimal {
                    precision: 10,
                    scale: 3,
                    coefficient: 1
                }
            ),
            "precision"
        );
        assert_eq!(
            render(
                1700,
                -1,
                NativeCell::Decimal {
                    precision: 38,
                    scale: 0,
                    coefficient: 7
                }
            ),
            "7"
        );
    }

    #[test]
    fn floats_round_trip_and_special_values_are_spelled_for_postgres() {
        assert_eq!(
            render(701, -1, NativeCell::Float64(1.5f64.to_bits())),
            "1.5"
        );
        assert_eq!(
            render(701, -1, NativeCell::Float64((-0.0f64).to_bits())),
            "-0.0"
        );
        assert_eq!(
            render(701, -1, NativeCell::Float64(f64::NAN.to_bits())),
            "NaN"
        );
        assert_eq!(
            render(701, -1, NativeCell::Float64(f64::INFINITY.to_bits())),
            "Infinity"
        );
        assert_eq!(
            render(701, -1, NativeCell::Float64(f64::NEG_INFINITY.to_bits())),
            "-Infinity"
        );
    }

    #[test]
    fn bytea_is_hex_behind_an_escaped_backslash() {
        assert_eq!(
            render(17, -1, NativeCell::Binary(vec![0, 255, 16])),
            "\\\\x00ff10"
        );
        assert_eq!(render(17, -1, NativeCell::Binary(Vec::new())), "\\\\x");
    }

    #[test]
    fn dates_and_timestamps_are_iso_with_micros() {
        assert_eq!(render(1082, -1, NativeCell::Date32(11_016)), "2000-02-29");
        assert_eq!(
            render(1114, -1, NativeCell::TimestampMicros(0)),
            "1970-01-01 00:00:00.000000"
        );
        assert_eq!(
            render(1114, -1, NativeCell::TimestampMicros(-1)),
            "1969-12-31 23:59:59.999999"
        );
        assert_eq!(
            render(
                1184,
                -1,
                NativeCell::TimestampMicrosUtc {
                    value: 946_684_800_123_456,
                    timezone: "UTC".into()
                }
            ),
            "2000-01-01 00:00:00.123456+00"
        );
        assert_eq!(
            refused(
                1184,
                -1,
                NativeCell::TimestampMicrosUtc {
                    value: 0,
                    timezone: "Asia/Tokyo".into()
                }
            ),
            "cell_type"
        );
    }

    #[test]
    fn duration_micros_uses_exact_signed_interval_text() {
        for value in [
            0,
            1,
            -1,
            90_000_000_001,
            -90_000_000_001,
            i64::MIN,
            i64::MAX,
        ] {
            assert_eq!(
                render(
                    1186,
                    -1,
                    NativeCell::Int {
                        bit_width: 64,
                        value
                    }
                ),
                format!("{value} microseconds")
            );
        }
        assert_eq!(
            refused(
                1186,
                -1,
                NativeCell::Int {
                    bit_width: 32,
                    value: 1
                }
            ),
            "cell_type"
        );
    }

    #[test]
    fn a_cell_of_the_wrong_kind_is_refused_not_converted() {
        assert_eq!(refused(20, -1, NativeCell::Utf8("12".into())), "cell_type");
        assert_eq!(
            refused(
                25,
                -1,
                NativeCell::Int {
                    bit_width: 64,
                    value: 12
                }
            ),
            "cell_type"
        );
        assert_eq!(refused(17, -1, NativeCell::Utf8("abc".into())), "cell_type");
        assert_eq!(
            refused(1082, -1, NativeCell::Date32(3_000_000)),
            "date_range"
        );
    }
}
