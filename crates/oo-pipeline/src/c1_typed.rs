//! Additive typed-key C1 vertical slice. One batch is one complete source
//! transaction (or one fill chunk). It is private staging, not publication.
use crate::{
    c1_key, c1_parts,
    native_arrow::{self, NativeCell},
    native_c1_apply::cell_text,
    target_installer::sha256_hex,
};
use serde::{Deserialize, Serialize};
use sqlx::{Connection, PgConnection};
use std::{collections::{BTreeMap, BTreeSet}, fs::File, io::Read, path::PathBuf};

pub const PROTOCOL: &str = "oo.c1.typed-batch/v3";
pub const ORDERED_PROTOCOL: &str = "oo.c1.typed-ordered-batch/v1";
pub const RECEIPT_PROTOCOL: &str = "oo.c1.typed-receipt/v3";
/// Resource bound for one decoded part, not the complete source transaction.
pub const MAX_ROWS: usize = 100_000;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub name: String,
    pub oid: i64,
    pub typmod: i32,
    pub nullable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSpec {
    pub columns: Vec<Column>,
    /// Column indexes, in SOURCE primary-key order (not column order).
    pub keys: Vec<usize>,
}

impl TableSpec {
    pub fn validate(&self) -> Result<()> {
        if self.columns.is_empty()
            || self.columns.len() > 1024
            || self.keys.is_empty()
            || self.keys.len() > 32
        {
            return Err("typed_schema");
        }
        let mut names = BTreeSet::new();
        for col in &self.columns {
            if col.name.is_empty()
                || col.name.len() > 256
                || col.name.contains('\0')
                || matches!(col.name.as_str(), "_oo_pos" | "_oo_op")
                || !names.insert(&col.name)
            {
                return Err("typed_schema");
            }
            c1_parts::logical_type_of_pg(col.oid, col.typmod).map_err(|_| "typed_schema")?;
        }
        let mut keys = BTreeSet::new();
        for &k in &self.keys {
            let c = self.columns.get(k).ok_or("typed_key_shape")?;
            if !keys.insert(k)
                || c.nullable
                || !(matches!(c.oid, 17 | 20 | 21 | 23 | 25 | 1043)
                    || c.oid == 1700 && c.typmod >= 4 && (c.typmod - 4) & 65535 == 0)
            {
                return Err("typed_key_type");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Lease {
    pub source: String,
    pub task: String,
    pub generation: i64,
    pub epoch: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub table: String,
    pub spec: TableSpec,
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
    pub rows: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub protocol: String,
    pub sequence: i64,
    pub previous_position: String,
    pub position: String,
    pub kind: String,
    pub parts: Vec<Part>,
}

fn position(s: &str) -> Result<u64> {
    let n = s.parse::<u64>().map_err(|_| "typed_position")?;
    if n.to_string() != s {
        return Err("typed_position");
    }
    Ok(n)
}

impl Batch {
    pub fn validate(&self) -> Result<()> {
        let prev = position(&self.previous_position)?;
        let pos = position(&self.position)?;
        let ordered = self.protocol == ORDERED_PROTOCOL;
        if (self.protocol != PROTOCOL && !ordered)
            || (ordered && (self.kind != "cdc" || self.parts.is_empty()))
            || self.sequence < 1
            || !(self.kind == "fill" && prev == pos && self.parts.len() == 1
                || self.kind == "cdc" && prev.checked_add(1) == Some(pos))
        {
            return Err("typed_batch");
        }
        let mut names = BTreeSet::new();
        for part in &self.parts {
            part.spec.validate()?;
            if part.table.is_empty()
                || part.table.len() > 512
                || part.table.contains('\0')
                || !names.insert(&part.table)
                || !part.path.is_absolute()
                || part
                    .path
                    .components()
                    .any(|c| c == std::path::Component::ParentDir)
                || part.bytes > c1_parts::PART_MAX_BYTES
                || part.bytes == 0
                || part.rows > MAX_ROWS
                || part.sha256.len() != 64
                || !part
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("typed_part");
            }
        }
        if ordered && !self.parts.iter().any(|part| part.rows != 0) {
            return Err("typed_budget");
        }
        Ok(())
    }

    /// Paths are local transport details, not replay identity.
    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        let mut value = serde_json::to_value(self).map_err(|_| "typed_batch")?;
        for part in value["parts"].as_array_mut().ok_or("typed_batch")? {
            part.as_object_mut().ok_or("typed_batch")?.remove("path");
        }
        Ok(sha256_hex(
            &serde_json::to_vec(&value).map_err(|_| "typed_batch")?,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub protocol: String,
    pub source: String,
    pub task: String,
    pub generation: i64,
    pub epoch: i32,
    pub sequence: i64,
    pub digest: String,
    pub position: String,
    pub replay: bool,
}

fn db(_: sqlx::Error) -> &'static str {
    "typed_target_refused"
}

pub(crate) fn field_matches(col: &Column, field: &native_arrow::DecodedField) -> bool {
    if col.nullable != field.nullable {
        return false;
    }
    match col.oid {
        20 | 21 | 23 => matches!(
            field.arrow_type.as_str(),
            "int8" | "int16" | "int32" | "int64" | "uint8" | "uint64"
        ),
        1700 => {
            field.arrow_type == "decimal128"
                && col.typmod >= 4
                && field.precision == Some((col.typmod - 4) >> 16)
                && field.scale == Some((col.typmod - 4) & 65535)
        }
        25 | 1042 | 1043 | 114 | 3802 => field.arrow_type == "utf8",
        17 => field.arrow_type == "binary",
        16 => field.arrow_type == "bool",
        700 | 701 => field.arrow_type == "float64",
        1082 => field.arrow_type == "date32",
        1114 => field.arrow_type == "timestamp_micros",
        1184 => {
            field.arrow_type == "timestamp_micros_utc" && field.timezone.as_deref() == Some("UTC")
        }
        1186 => field.arrow_type == "duration_micros",
        _ => false,
    }
}

pub async fn open(
    conn: &mut PgConnection,
    lease: &Lease,
    name: &str,
    spec: &TableSpec,
) -> Result<()> {
    spec.validate()?;
    sqlx::query_scalar::<_, String>("SELECT _oo.c1_typed_open($1,$2::uuid,$3,$4,$5,$6::jsonb)")
        .bind(&lease.source)
        .bind(&lease.task)
        .bind(lease.generation)
        .bind(lease.epoch)
        .bind(name)
        .bind(serde_json::to_string(spec).map_err(|_| "typed_schema")?)
        .fetch_one(conn)
        .await
        .map_err(db)?;
    Ok(())
}

/// Decode and validate one bounded part. The sealed file is read once;
/// the same checked bytes supply COPY (no pathname re-open).
pub fn copy_bytes(part: &Part, kind: &str, pos: &str) -> Result<Vec<u8>> {
    copy_part(part, kind, pos, false).map(|(bytes, _)| bytes)
}

fn copy_part(part: &Part, kind: &str, pos: &str, ordered: bool) -> Result<(Vec<u8>, usize)> {
    part.spec.validate()?;
    position(pos)?;
    if !matches!(kind, "fill" | "cdc") || (ordered && kind != "cdc") {
        return Err("typed_batch");
    }
    let file = File::open(&part.path).map_err(|_| "typed_part_read")?;
    let meta = file.metadata().map_err(|_| "typed_part_read")?;
    if !meta.is_file() || meta.len() != part.bytes || part.bytes > c1_parts::PART_MAX_BYTES {
        return Err("typed_part_bytes");
    }
    let mut bytes = Vec::new();
    file.take(c1_parts::PART_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "typed_part_read")?;
    if bytes.len() as u64 != part.bytes || sha256_hex(&bytes) != part.sha256 {
        return Err("typed_part_digest");
    }
    let decoded = native_arrow::decode_arrow_stream(&bytes).map_err(|_| "typed_arrow")?;
    let n = part.spec.columns.len();
    let cdc = kind == "cdc";
    if decoded.rows != part.rows
        || decoded.rows > MAX_ROWS
        || decoded.columns.len() != n + if cdc { 2 } else { 0 }
        || decoded.fields.iter().take(n).map(|f| &f.name).ne(part
            .spec
            .columns
            .iter()
            .map(|c| &c.name))
        || part
            .spec
            .columns
            .iter()
            .zip(&decoded.fields)
            .any(|(c, f)| !field_matches(c, f))
    {
        return Err("typed_arrow_schema");
    }
    if cdc
        && (decoded.fields[n].name != "_oo_pos"
            || decoded.fields[n + 1].name != "_oo_op"
            || decoded.fields[n].arrow_type != "decimal128"
            || decoded.fields[n].precision != Some(38)
            || decoded.fields[n].scale != Some(0)
            || decoded.fields[n + 1].arrow_type != "utf8"
            || decoded.fields[n].nullable
            || decoded.fields[n + 1].nullable)
    {
        return Err("typed_arrow_markers");
    }
    let mut keys = BTreeSet::new();
    let mut last = BTreeMap::new();
    let mut out = Vec::new();
    for row in 0..decoded.rows {
        let key = part
            .spec
            .keys
            .iter()
            .map(|&k| c1_key::component(&decoded.columns[k][row]))
            .collect::<Result<Vec<_>>>()?;
        let id = c1_key::identity(&key)?;
        // The codec supports longer identities, but this slice uses a bytea
        // B-tree. Refuse before COPY instead of hitting a page-dependent limit.
        if id.len() > 2000 {
            return Err("typed_key_storage_budget");
        }
        if !ordered && !keys.insert(id.clone()) {
            return Err("typed_duplicate_key");
        }
        let op = if cdc {
            match &decoded.columns[n][row] {
                NativeCell::Decimal {
                    coefficient,
                    scale: 0,
                    ..
                } if coefficient.to_string() == pos => (),
                _ => return Err("typed_transaction_position"),
            }
            match &decoded.columns[n + 1][row] {
                NativeCell::Utf8(s) if s == "upsert" || s == "delete" => s.as_str(),
                _ => return Err("typed_operation"),
            }
        } else {
            "upsert"
        };
        let start = out.len();
        let ordered_key = ordered.then(|| id.clone());
        cell_text(&mut out, 17, -1, &NativeCell::Binary(id))?;
        out.push(b'\t');
        out.extend_from_slice(op.as_bytes());
        for (k, col) in part.spec.columns.iter().enumerate() {
            let cell = &decoded.columns[k][row];
            if op == "upsert" && !col.nullable && *cell == NativeCell::Null {
                return Err("typed_null");
            }
            out.push(b'\t');
            cell_text(&mut out, col.oid, col.typmod, cell)?;
        }
        out.push(b'\n');
        if out.len() > 128 * 1024 * 1024 {
            return Err("typed_copy_budget");
        }
        // Validate every source event; only private final-state COPY is reduced.
        if let Some(key) = ordered_key {
            last.insert(key, (start, out.len()));
        }
    }
    let copied_rows = if ordered {
        let mut survivors: Vec<_> = last.into_values().collect();
        survivors.sort_by_key(|(start, _)| *start);
        let count = survivors.len();
        let mut written = 0;
        for (start, end) in survivors {
            out.copy_within(start..end, written);
            written += end - start;
        }
        out.truncate(written);
        count
    } else { decoded.rows };
    Ok((out, copied_rows))
}

/// All tables, tombstones and the transaction receipt commit atomically.
/// This returns a TARGET receipt, never an ACK.
pub async fn apply(conn: &mut PgConnection, lease: &Lease, batch: &Batch) -> Result<Receipt> {
    apply_inner(conn,lease,batch,false,None,None,false).await
}

/// Formal driver: hold the existing real jobs claim for the entire target TX.
pub async fn apply_claimed(conn: &mut PgConnection, lease: &Lease, batch: &Batch, stamp:Option<&str>) -> Result<Receipt> {
    if batch.kind=="cdc" && stamp.is_none() {return Err("typed_commit_time");}
    apply_inner(conn,lease,batch,true,stamp,None,false).await
}

/// Oracle uses the same typed COPY/tombstone transaction, but checkpoint
/// evidence is opaque and never inferred from the target apply ordinal.
pub async fn apply_oracle_claimed(conn:&mut PgConnection,lease:&Lease,batch:&Batch,position:Option<&crate::c1_source_position::OraclePosition>)->Result<Receipt> {
    let bound=position.map(|p|p.bound()).transpose()?;
    if batch.kind=="cdc" && bound.is_none() {return Err("oracle_checkpoint_required");}
    if batch.kind=="fill" && bound.is_some() {return Err("oracle_snapshot_not_cdc");}
    apply_inner(conn,lease,batch,true,None,bound.as_ref(),true).await
}

pub async fn apply_oracle_buffer_claimed(conn:&mut PgConnection,lease:&Lease,batch:&Batch,position:&crate::c1_source_position::CapturePosition)->Result<Receipt>{
    let bound=position.bound(&lease.source,batch)?;
    apply_inner(conn,lease,batch,true,None,Some(&bound),true).await
}

pub async fn apply_oracle_buffer_context_claimed(
    conn:&mut PgConnection,lease:&Lease,batch:&Batch,
    context:&crate::c1_buffer_position::ArtifactRef,position:Option<&crate::c1_buffer_position::Position>,
)->Result<Receipt>{
    let bound=crate::c1_buffer_position::bound(context,&lease.source,batch,position)?;
    apply_inner(conn,lease,batch,true,None,Some(&bound),true).await
}

async fn apply_inner(conn: &mut PgConnection, lease: &Lease, batch: &Batch, claimed:bool,stamp:Option<&str>,oracle:Option<&serde_json::Value>,oracle_engine:bool) -> Result<Receipt> {
    let digest = batch.digest()?;
    let mut tx = conn.begin().await.map_err(db)?;
    let held=if claimed {Some(crate::native_update::lease_hold(&mut tx,&lease.task,Some(lease.generation)).await?.0)}else{None};
    if claimed {
        sqlx::query("SELECT _oo.c1_typed_assert_engine($1,$2::uuid,$3,$4,$5)")
            .bind(&lease.source).bind(&lease.task).bind(lease.generation).bind(lease.epoch)
            .bind(if oracle_engine {"oracle"}else{"mysql"}).execute(&mut *tx).await.map_err(db)?;
        if oracle_engine {
            sqlx::query("SELECT _oo.c1_typed_assert_buffer_context($1,$2::uuid,$3,$4,$5::jsonb)")
                .bind(&lease.source).bind(&lease.task).bind(lease.generation).bind(lease.epoch)
                .bind(serde_json::to_string(&oracle).map_err(|_|"oracle_position")?)
                .execute(&mut *tx).await.map_err(db)?;
        }
    }
    let started=std::time::Instant::now();
    let mut keeper=crate::native_update::LeaseKeeper::for_lease(crate::native_update::BASELINE_LEASE_TTL);
    let fresh: bool = sqlx::query_scalar(
        "SELECT _oo.c1_typed_begin($1,$2::uuid,$3,$4,$5,$6,$7::numeric,$8::numeric,$9)",
    )
    .bind(&lease.source)
    .bind(&lease.task)
    .bind(lease.generation)
    .bind(lease.epoch)
    .bind(batch.sequence)
    .bind(&digest)
    .bind(&batch.previous_position)
    .bind(&batch.position)
    .bind(&batch.kind)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    for part in &batch.parts {
        if let Some(held)=&held {crate::native_update::renew_if_due(&mut tx,held,&mut keeper,started).await?;}
        // Keep only this part's COPY payload alive. A bad later part rolls back
        // earlier COPY/apply work; even receipt replays validate every input.
        let (bytes, copied_rows) =
            copy_part(part, &batch.kind, &batch.position, batch.protocol == ORDERED_PROTOCOL)?;
        if fresh {
            // open() is idempotent but refuses a schema drift, including key order.
            let stage: String =
                sqlx::query_scalar("SELECT _oo.c1_typed_open($1,$2::uuid,$3,$4,$5,$6::jsonb)")
                    .bind(&lease.source)
                    .bind(&lease.task)
                    .bind(lease.generation)
                    .bind(lease.epoch)
                    .bind(&part.table)
                    .bind(serde_json::to_string(&part.spec).map_err(|_| "typed_schema")?)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db)?;
            if stage.len() != 36
                || !stage.starts_with("c3s_")
                || !stage[4..].bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("typed_stage_name");
            }
            let columns = (0..part.spec.columns.len())
                .map(|i| format!(",c{i}"))
                .collect::<String>();
            let query = format!("COPY _oo.{stage}(k,op{columns}) FROM STDIN WITH (FORMAT text)");
            let mut copy = tx.copy_in_raw(&query).await.map_err(db)?;
            for chunk in bytes.chunks(256 * 1024) {
                copy.send(chunk).await.map_err(db)?;
            }
            if copy.finish().await.map_err(db)? != copied_rows as u64 {
                return Err("typed_copy_count");
            }
            sqlx::query("SELECT _oo.c1_typed_apply_table($1,$2::uuid,$3,$4,$5,$6,$7::numeric,$8)")
                .bind(&lease.source)
                .bind(&lease.task)
                .bind(lease.generation)
                .bind(lease.epoch)
                .bind(&part.table)
                .bind(&batch.kind)
                .bind(&batch.position)
                .bind(copied_rows as i64)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
    }
    if fresh {
        sqlx::query(
            "SELECT _oo.c1_typed_finish($1,$2::uuid,$3,$4,$5,$6,$7::numeric,$8::numeric,$9)",
        )
        .bind(&lease.source)
        .bind(&lease.task)
        .bind(lease.generation)
        .bind(lease.epoch)
        .bind(batch.sequence)
        .bind(&digest)
        .bind(&batch.previous_position)
        .bind(&batch.position)
        .bind(&batch.kind)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    }
    if let Some(position)=oracle.filter(|_|batch.kind=="cdc") {
        let sql=if position["protocol"]=="oo.c1.oracle-buffer-cursor/v1" {
            "SELECT _oo.c1_typed_record_buffer($1,$2::uuid,$3,$4,$5,$6,$7::jsonb)"
        }else{"SELECT _oo.c1_typed_record_oracle_position($1,$2::uuid,$3,$4,$5,$6,$7::jsonb)"};
        sqlx::query(sql)
            .bind(&lease.source).bind(&lease.task).bind(lease.generation).bind(lease.epoch)
            .bind(batch.sequence).bind(&digest).bind(serde_json::to_string(position).map_err(|_|"oracle_position")?)
            .execute(&mut *tx).await.map_err(db)?;
    } else if claimed && !oracle_engine && batch.kind=="cdc" {
        sqlx::query("SELECT _oo.c1_typed_commit_time($1,$2::uuid,$3,$4,$5,$6,$7)")
            .bind(&lease.source).bind(&lease.task).bind(lease.generation).bind(lease.epoch)
            .bind(batch.sequence).bind(&digest).bind(stamp.ok_or("typed_commit_time")?)
            .execute(&mut *tx).await.map_err(db)?;
    }
    if let Some(held)=&held {crate::native_update::renew_if_due(&mut tx,held,&mut keeper,started).await?;}
    tx.commit().await.map_err(db)?;
    Ok(Receipt {
        protocol: RECEIPT_PROTOCOL.into(),
        source: lease.source.clone(),
        task: lease.task.clone(),
        generation: lease.generation,
        epoch: lease.epoch,
        sequence: batch.sequence,
        digest,
        position: batch.position.clone(),
        replay: !fresh,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordered_fixture(name: &str) -> Part {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/c1-ordered/manifest.json")).unwrap();
        let entry = &manifest["fixtures"][name];
        Part {
            table: "UNIT.ORDERED".into(),
            spec: serde_json::from_value(entry["spec"].clone()).unwrap(),
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/c1-ordered/{name}.arrow")),
            sha256: entry["sha256"].as_str().unwrap().into(),
            bytes: entry["bytes"].as_u64().unwrap(),
            rows: entry["rows"].as_u64().unwrap() as usize,
        }
    }

    #[test]
    fn ordered_cdc_validates_raw_events_and_copies_only_last_operation_per_typed_key() {
        let part = ordered_fixture("ordered");
        assert_eq!(copy_bytes(&part, "cdc", "1").unwrap_err(), "typed_duplicate_key");
        let (bytes, copied) = copy_part(&part, "cdc", "1", true).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!((part.rows, copied, text.lines().count()), (6, 2, 2));
        assert!(text.contains("\tupsert\tk\t1\tnew"));
        assert!(text.contains("\tdelete\tz\t2\t\\N\n"));
        assert!(!text.contains("old") && !text.contains("temporary") && !text.contains("changed"));
        assert_eq!(copy_part(&ordered_fixture("invalid-operation"), "cdc", "1", true).unwrap_err(),
            "typed_operation");
        assert_eq!(copy_part(&ordered_fixture("wrong-position"), "cdc", "1", true).unwrap_err(),
            "typed_transaction_position");
        let mut false_count = part;
        false_count.rows = 2;
        assert_eq!(copy_part(&false_count, "cdc", "1", true).unwrap_err(), "typed_arrow_schema");
    }

    #[test]
    fn ordered_protocol_is_explicit_cdc_only_and_keeps_raw_row_limits() {
        let mut batch = Batch { protocol: PROTOCOL.into(), sequence: 1,
            previous_position: "0".into(), position: "1".into(), kind: "cdc".into(),
            parts: vec![ordered_fixture("ordered")] };
        let legacy_digest = batch.digest().unwrap();
        batch.protocol = ORDERED_PROTOCOL.into();
        batch.validate().unwrap();
        assert_ne!(legacy_digest, batch.digest().unwrap());
        batch.parts[0].rows = MAX_ROWS + 1;
        assert!(batch.validate().is_err());
        batch.parts = vec![ordered_fixture("duplicate-fill")];
        batch.kind = "fill".into();
        batch.position = "0".into();
        assert!(batch.validate().is_err());
        batch.protocol = PROTOCOL.into();
        batch.validate().unwrap();
        assert_eq!(copy_bytes(&batch.parts[0], "fill", "0").unwrap_err(), "typed_duplicate_key");
    }

    #[test]
    fn transaction_cardinality_is_not_a_part_resource_budget() {
        let mut batch = Batch { protocol: PROTOCOL.into(), sequence: 1,
            previous_position: "0".into(), position: "1".into(), kind: "cdc".into(),
            parts: (0..33).map(|i| {
                let mut part = ordered_fixture("ordered");
                part.table = format!("UNIT.T{i}");
                part.rows = MAX_ROWS;
                part
            }).collect() };
        batch.validate().unwrap();
        let digest = batch.digest().unwrap();
        batch.protocol = ORDERED_PROTOCOL.into();
        batch.validate().unwrap();
        assert_ne!(batch.digest().unwrap(), digest);
        batch.parts[32].rows += 1;
        assert_eq!(batch.validate(), Err("typed_part"));
        batch.parts[32].rows = MAX_ROWS;
        batch.parts[32].table = batch.parts[0].table.clone();
        assert_eq!(batch.validate(), Err("typed_part"));
    }

    #[test]
    fn key_schema_is_closed_not_untyped_json() {
        let spec = TableSpec {
            columns: vec![Column {
                name: "id".into(),
                oid: 1043,
                typmod: -1,
                nullable: false,
            }],
            keys: vec![0],
        };
        spec.validate().unwrap();
        let mut bad = spec.clone();
        bad.keys.push(0);
        assert!(bad.validate().is_err());
        bad = spec.clone();
        bad.columns[0].nullable = true;
        assert!(bad.validate().is_err());
        bad = spec.clone();
        bad.columns[0].oid = 701;
        assert!(bad.validate().is_err());
        assert!(
            serde_json::from_str::<Column>(
                r#"{"name":"x","oid":20,"typmod":-1,"nullable":false,"surprise":1}"#
            )
            .is_err()
        );
    }

    #[test]
    fn metadata_type_and_position_are_checked_even_for_null_or_empty_parts() {
        let col = Column {
            name: "x".into(),
            oid: 20,
            typmod: -1,
            nullable: true,
        };
        let mut f = native_arrow::DecodedField {
            name: "x".into(),
            nullable: true,
            arrow_type: "utf8".into(),
            precision: None,
            scale: None,
            timezone: None,
        };
        assert!(!field_matches(&col, &f));
        f.arrow_type = "int64".into();
        assert!(field_matches(&col, &f));
        for invalid in ["01", "-1", "1.0", "18446744073709551616"] {
            assert!(position(invalid).is_err());
        }
        let empty = Batch {
            protocol: PROTOCOL.into(),
            sequence: 1,
            previous_position: "1".into(),
            position: "2".into(),
            kind: "cdc".into(),
            parts: vec![],
        };
        assert!(empty.validate().is_ok());
        let mut bad = empty;
        bad.position = "1".into();
        assert!(bad.validate().is_err());
    }
}
