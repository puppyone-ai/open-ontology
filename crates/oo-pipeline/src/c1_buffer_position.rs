//! Immutable Oracle buffer consumption. This is never a source resume offset.
use crate::{
    c1_source_position::{CapturePosition, Identity, OpaqueOffset, hex64},
    c1_typed::Batch,
    target_installer::sha256_hex,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
};

type Result<T> = std::result::Result<T, &'static str>;
const SMALL: u64 = 1024 * 1024;
const EVIDENCE: u64 = 32 * SMALL;
pub const POSITION: &str = "oo.c1.oracle-buffer-position/v1";
pub const SCOPE: &str = "oo.source-admission.c1-oracle-buffer-scope/v1";
pub const CODEC: &str = "oracle-capture-sequence/v1";

/// Public final-load scope: the source recovery files and capture ordinal stay
/// distinct even after the existing native watermark is advanced.
pub fn validate_scope(scope: &Value) -> Result<()> {
    let b = &scope["buffer"];
    let identity: Identity =
        serde_json::from_value(b["source_identity"].clone()).map_err(|_| "oracle_buffer_scope")?;
    let cursor: Cursor =
        serde_json::from_value(b["final_cursor"].clone()).map_err(|_| "oracle_buffer_scope")?;
    let ordinal = cursor.ordinal()?;
    let prefix = format!(
        "oracle-buffer:{}:",
        b["context_sha256"].as_str().ok_or("oracle_buffer_scope")?
    );
    let start = scope["l0"]
        .as_str()
        .and_then(|s| s.strip_prefix(&prefix))
        .and_then(|s| {
            s.parse::<u64>()
                .ok()
                .filter(|n| n.to_string() == s && *n <= ordinal)
        })
        .ok_or("oracle_buffer_scope")?;
    if scope["protocol"] != SCOPE
        || scope["engine"] != "oracle"
        || identity.engine() != "oracle"
        || scope["lineage"] != identity.measured_id()?
        || scope["source_identity"]["engine"] != "oracle"
        || scope["source_identity"]["measured_id"] != scope["lineage"]
        || scope["schema_digest"] != b["typed_schema_sha256"]
        || scope["complete"] != true
        || scope["consistency_grade"] != "T"
        || scope["cross_table_consistent"] != true
        || b["checkpoint_kind"] != "buffer_consumption"
        || b["source_offset_write"] != false
        || [
            "context_sha256",
            "bundle_sha256",
            "spool_sha256",
            "typed_schema_sha256",
            "connector_offsets_sha256",
            "connector_history_sha256",
        ]
        .into_iter()
        .any(|key| !b[key].as_str().is_some_and(hex64))
        || (ordinal > 0 && !b["commit_scn"].as_str().is_some_and(scn))
        || (ordinal == 0 && !b["commit_scn"].is_null())
        || (start > 0 && start == ordinal)
        || scope["l_end"]
            != format!(
                "oracle-buffer:{}:{ordinal}",
                b["context_sha256"].as_str().ok_or("oracle_buffer_scope")?
            )
    {
        return Err("oracle_buffer_scope");
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

impl ArtifactRef {
    pub fn verify(&self, limit: u64) -> Result<()> {
        if !self.path.is_absolute()
            || self
                .path
                .components()
                .any(|c| c == std::path::Component::ParentDir)
            || !hex64(&self.sha256)
            || self.bytes == 0
            || self.bytes > limit
        {
            return Err("buffer_artifact");
        }
        let mut file = File::open(&self.path).map_err(|_| "buffer_artifact")?;
        let meta = file.metadata().map_err(|_| "buffer_artifact")?;
        if !meta.is_file() || meta.len() != self.bytes {
            return Err("buffer_artifact");
        }
        let mut hash = Sha256::new();
        let mut seen = 0u64;
        let mut bytes = [0; 64 * 1024];
        loop {
            let n = file.read(&mut bytes).map_err(|_| "buffer_artifact")?;
            if n == 0 {
                break;
            }
            seen += n as u64;
            if seen > self.bytes {
                return Err("buffer_artifact");
            }
            hash.update(&bytes[..n]);
        }
        if seen != self.bytes || format!("{:x}", hash.finalize()) != self.sha256 {
            return Err("buffer_artifact");
        }
        let after = file.metadata().map_err(|_| "buffer_artifact")?;
        if after.len() != meta.len() || after.modified().ok() != meta.modified().ok() {
            return Err("buffer_artifact");
        }
        Ok(())
    }

    fn json_lines(&self, mut inspect: impl FnMut(Value) -> Result<()>) -> Result<()> {
        // Durable evidence is disk-budgeted by capture. Only one small event
        // record is resident, regardless of complete transaction cardinality.
        const RECORD_BYTES: u64 = 64 * 1024;
        self.verify(u64::MAX)?;
        let file = File::open(&self.path).map_err(|_| "buffer_artifact")?;
        let before = file.metadata().map_err(|_| "buffer_artifact")?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let mut bytes = Vec::new();
        let mut hash = Sha256::new();
        let mut seen = 0u64;
        loop {
            bytes.clear();
            let n = reader
                .by_ref()
                .take(RECORD_BYTES + 1)
                .read_until(b'\n', &mut bytes)
                .map_err(|_| "buffer_events")?;
            if n == 0 {
                break;
            }
            if n as u64 > RECORD_BYTES || bytes.last() != Some(&b'\n') {
                return Err("buffer_event_record");
            }
            seen = seen.checked_add(n as u64).ok_or("buffer_artifact")?;
            if seen > self.bytes {
                return Err("buffer_artifact");
            }
            hash.update(&bytes);
            inspect(serde_json::from_slice(&bytes).map_err(|_| "buffer_events")?)?;
        }
        let after = reader.get_ref().metadata().map_err(|_| "buffer_artifact")?;
        if seen != self.bytes
            || format!("{:x}", hash.finalize()) != self.sha256
            || before.len() != self.bytes
            || after.len() != before.len()
            || after.modified().ok() != before.modified().ok()
        {
            return Err("buffer_artifact");
        }
        Ok(())
    }

    fn read(&self, limit: u64) -> Result<Vec<u8>> {
        self.verify(limit)?;
        let mut bytes = Vec::new();
        File::open(&self.path)
            .map_err(|_| "buffer_artifact")?
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "buffer_artifact")?;
        if bytes.len() as u64 != self.bytes || sha256_hex(&bytes) != self.sha256 {
            return Err("buffer_artifact");
        }
        Ok(bytes)
    }

    pub(crate) fn document(&self) -> Result<Value> {
        serde_json::from_slice(&self.read(SMALL)?).map_err(|_| "buffer_document")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub transaction_ordinal: String,
    pub transaction_id: Option<String>,
}

impl Cursor {
    fn ordinal(&self) -> Result<u64> {
        let n = self
            .transaction_ordinal
            .parse::<u64>()
            .map_err(|_| "buffer_cursor")?;
        if n.to_string() != self.transaction_ordinal
            || (n == 0) != self.transaction_id.is_none()
            || self.transaction_id.as_ref().is_some_and(|id| !token(id))
        {
            return Err("buffer_cursor");
        }
        Ok(n)
    }
}

fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-$#".contains(&b))
}
fn scn(s: &str) -> bool {
    !s.is_empty() && s.len() <= 38 && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
}
fn value_hash(value: &Value) -> Result<String> {
    Ok(sha256_hex(
        &serde_json::to_vec(value).map_err(|_| "buffer_document")?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceResume {
    kind: String,
    final_connector_offset: ArtifactRef,
    connector_history: ArtifactRef,
    target_ack_updates_vendor_offset: bool,
    intermediate_vendor_resume_offsets: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    protocol: String,
    engine: String,
    source_id: String,
    source_identity: Identity,
    bundle: ArtifactRef,
    raw_spool: ArtifactRef,
    typed_schema_sha256: String,
    snapshot_scn: String,
    initial_cursor: Cursor,
    source_resume: SourceResume,
    cursor_semantics: String,
    #[serde(default)]
    rollover: Option<ArtifactRef>,
}

fn context(
    reference: &ArtifactRef,
    source: &str,
    verify_capture: bool,
) -> Result<(Context, CapturePosition)> {
    let ctx: Context =
        serde_json::from_value(reference.document()?).map_err(|_| "buffer_context")?;
    let capture = CapturePosition {
        protocol: "oo.c1.oracle-buffer-cursor/v1".into(),
        bundle: OpaqueOffset {
            path: ctx.bundle.path.clone(),
            sha256: ctx.bundle.sha256.clone(),
            bytes: ctx.bundle.bytes,
        },
    };
    let doc = capture.document()?;
    if ctx.protocol != "oo.c1.oracle-buffer-context/v1"
        || ctx.engine != "oracle"
        || ctx.source_id != source
        || doc["source_id"] != source
        || ctx.source_identity != capture.identity()?
        || ctx.source_identity.engine() != "oracle"
        || (ctx.rollover.is_none() && ctx.initial_cursor.ordinal()? != 0)
        || !scn(&ctx.snapshot_scn)
        || doc["boundary"]["initial_position"] != ctx.initial_cursor.transaction_ordinal
        || doc["boundary"]["snapshot_scn"] != ctx.snapshot_scn
        || doc["boundary"]["spool_sha256"] != ctx.raw_spool.sha256
        || doc["boundary"]["connector_offsets_sha256"]
            != ctx.source_resume.final_connector_offset.sha256
        || doc["boundary"]["connector_history_sha256"] != ctx.source_resume.connector_history.sha256
        || ctx.typed_schema_sha256 != value_hash(&doc["typed_tables"])?
        || ctx.source_resume.kind != "vendor_offset_retained_separately"
        || ctx.source_resume.target_ack_updates_vendor_offset
        || ctx
            .source_resume
            .intermediate_vendor_resume_offsets
            .is_empty()
        || ctx.cursor_semantics.is_empty()
    {
        return Err("buffer_context");
    }
    if verify_capture {
        // Only opening consumes the raw capture. Applies read the pinned bundle
        // and per-transaction evidence, never a live or mutable source offset.
        ctx.raw_spool.verify(u64::MAX)?; // Streamed disk artifact, not a RAM-sized document.
        ctx.source_resume.final_connector_offset.verify(SMALL)?;
        ctx.source_resume.connector_history.verify(EVIDENCE)?;
    }
    Ok((ctx, capture))
}

fn binding(reference: &ArtifactRef, ctx: &Context, mut bound: Value) -> Value {
    bound["context_sha256"] = json!(reference.sha256);
    bound["typed_schema_sha256"] = json!(ctx.typed_schema_sha256);
    bound["source_identity"] = json!(ctx.source_identity);
    bound["source_offset_write"] = json!(false);
    bound["buffer_position_protocol"] = json!(POSITION);
    bound
}

pub fn open(reference: &ArtifactRef, source: &str, tables: &Value, first: &Batch) -> Result<Value> {
    let (ctx, capture) = context(reference, source, true)?;
    if ctx.rollover.is_some()
        || first.sequence != 1
        || first.kind != "fill"
        || first.position != "0"
        || capture.document()?["typed_tables"] != *tables
    {
        return Err("buffer_baseline");
    }
    Ok(binding(reference, &ctx, capture.bound(source, first)?))
}

fn initial_fill_batches(document: &Value, bytes: &[u8]) -> Result<(Value, Value)> {
    let specs = document["typed_tables"]
        .as_object()
        .filter(|t| !t.is_empty())
        .ok_or("oracle_fill_tables")?;
    let mut tables = BTreeMap::<String, Value>::new();
    let mut batches = Vec::new();
    for line in std::str::from_utf8(bytes)
        .map_err(|_| "oracle_fill_batches")?
        .lines()
    {
        let frame: Value = serde_json::from_str(line).map_err(|_| "oracle_fill_batches")?;
        let batch: Batch =
            serde_json::from_value(frame["batch"].clone()).map_err(|_| "oracle_fill_batches")?;
        batch.validate()?;
        if frame["kind"] != "typed_batch"
            || batch.kind != "fill"
            || batch.position != "0"
            || batch.previous_position != "0"
            || batch.sequence != batches.len() as i64 + 1
        {
            return Err("oracle_fill_sequence");
        }
        for part in &batch.parts {
            if specs.get(&part.table)
                != Some(&serde_json::to_value(&part.spec).map_err(|_| "oracle_fill_tables")?)
            {
                return Err("oracle_fill_tables");
            }
            let entry = tables
                .entry(part.table.clone())
                .or_insert(json!({"rows":0}));
            let rows = entry["rows"]
                .as_u64()
                .ok_or("oracle_fill_rows")?
                .checked_add(part.rows as u64)
                .ok_or("oracle_fill_rows")?;
            *entry = json!({"last_sequence":batch.sequence,"rows":rows});
        }
        batches.push(json!({"sequence":batch.sequence,"digest":batch.digest()?}));
    }
    let rows = tables
        .values()
        .try_fold(0u64, |n, t| n.checked_add(t["rows"].as_u64()?))
        .ok_or("oracle_fill_rows")?;
    if batches.is_empty()
        || document["batch_count"].as_u64() != Some(batches.len() as u64)
        || tables.len() != specs.len()
        || document["final_rows"].as_u64() != Some(rows)
    {
        return Err("oracle_fill_coverage");
    }
    Ok((json!(batches), json!(tables)))
}

/// Rebuild the resume plan from the hash-pinned captured batches, not caller counts.
pub fn open_initial_fill(
    reference: &ArtifactRef,
    index: &ArtifactRef,
    source: &str,
    tables: &Value,
    first: &Batch,
    supplied_plan: &Value,
    config_digest: &str,
) -> Result<Value> {
    if supplied_plan["protocol"] == "oo.oracle.initial-mixed/v1" {
        return open_initial_mixed(
            reference,
            index,
            source,
            tables,
            first,
            supplied_plan,
            config_digest,
        );
    }
    let mut bound = open(reference, source, tables, first)?;
    let context = reference.document()?;
    let capture: ArtifactRef =
        serde_json::from_value(context["bundle"].clone()).map_err(|_| "oracle_fill_bundle")?;
    let document = capture.document()?;
    let index_document = index.document()?;
    let cursor = json!({"transaction_ordinal":"0","transaction_id":null});
    if !hex64(config_digest)
        || context["initial_cursor"] != cursor
        || index_document["protocol"] != "oo.oracle.buffer-index/v1"
        || index_document["context"]
            != serde_json::to_value(reference).map_err(|_| "oracle_fill_index")?
        || index_document["final_cursor"] != cursor
        || index_document["transactions"] != json!([])
        || document["boundary"]["final_position"] != "0"
        || document["checkpoints"] != json!([])
    {
        return Err("oracle_fill_index");
    }
    let path = capture
        .path
        .parent()
        .ok_or("oracle_fill_bundle")?
        .join("batches.jsonl");
    let batch_ref = ArtifactRef {
        bytes: std::fs::metadata(&path)
            .map_err(|_| "oracle_fill_batches")?
            .len(),
        path,
        sha256: document["artifacts"]["batches.jsonl"]
            .as_str()
            .ok_or("oracle_fill_batches")?
            .into(),
    };
    let (batches, table_plan) = initial_fill_batches(&document, &batch_ref.read(EVIDENCE)?)?;
    let plan = json!({"protocol":"oo.oracle.initial-fill/v1","source_id":source,
        "config_digest":config_digest,"typed_schema_sha256":context["typed_schema_sha256"],
        "buffer_index":index,"buffer_context":reference,"bundle_sha256":capture.sha256,
        "batch_count":document["batch_count"],"batches":batches,"tables":table_plan});
    if *supplied_plan != plan {
        return Err("oracle_fill_plan");
    }
    bound["buffer_index"] = json!(index);
    bound["initial_fill"] = plan;
    Ok(bound)
}

/// Mixed initial recovery is explicit; the legacy pure-fill plan remains unchanged.
pub fn open_initial_mixed(
    reference: &ArtifactRef,
    index: &ArtifactRef,
    source: &str,
    tables: &Value,
    first: &Batch,
    supplied_plan: &Value,
    config_digest: &str,
) -> Result<Value> {
    let mut binding = open(reference, source, tables, first)?;
    let ctx = reference.document()?;
    let capture: ArtifactRef =
        serde_json::from_value(ctx["bundle"].clone()).map_err(|_| "oracle_mixed_bundle")?;
    let doc = capture.document()?;
    let idx = index.document()?;
    let txs = idx["transactions"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or("oracle_mixed_index")?;
    if !hex64(config_digest)
        || doc["boundary"]["mode"] != "oracle_scn_snapshot_and_log"
        || ctx["initial_cursor"] != json!({"transaction_ordinal":"0","transaction_id":null})
        || idx["protocol"] != "oo.oracle.buffer-index/v1"
        || idx["context"] != serde_json::to_value(reference).map_err(|_| "oracle_mixed_index")?
    {
        return Err("oracle_mixed_binding");
    }
    let path = capture
        .path
        .parent()
        .ok_or("oracle_mixed_bundle")?
        .join("batches.jsonl");
    let raw = ArtifactRef {
        bytes: std::fs::metadata(&path)
            .map_err(|_| "oracle_mixed_batches")?
            .len(),
        path,
        sha256: doc["artifacts"]["batches.jsonl"]
            .as_str()
            .ok_or("oracle_mixed_batches")?
            .into(),
    }
    .read(EVIDENCE)?;
    let specs = doc["typed_tables"]
        .as_object()
        .filter(|m| !m.is_empty())
        .ok_or("oracle_mixed_tables")?;
    let mut table_plan = BTreeMap::<String, Value>::new();
    let mut batches = Vec::new();
    let mut fills = 0usize;
    let mut pos = 0usize;
    let mut cursor = ctx["initial_cursor"].clone();
    for line in std::str::from_utf8(&raw)
        .map_err(|_| "oracle_mixed_batches")?
        .lines()
    {
        let frame: Value = serde_json::from_str(line).map_err(|_| "oracle_mixed_batches")?;
        let batch: Batch =
            serde_json::from_value(frame["batch"].clone()).map_err(|_| "oracle_mixed_batches")?;
        batch.validate()?;
        if frame["kind"] != "typed_batch" || batch.sequence != batches.len() as i64 + 1 {
            return Err("oracle_mixed_sequence");
        }
        let mut entry = json!({"sequence":batch.sequence,"digest":batch.digest()?,"kind":batch.kind,
            "previous_position":batch.previous_position,"position":batch.position});
        if batch.kind == "fill" {
            if pos != 0 || batch.position != "0" || batch.previous_position != "0" {
                return Err("oracle_mixed_fill_after_cdc");
            }
            fills += 1;
            for part in &batch.parts {
                if specs.get(&part.table)
                    != Some(&serde_json::to_value(&part.spec).map_err(|_| "oracle_mixed_tables")?)
                {
                    return Err("oracle_mixed_tables");
                }
                let table = table_plan
                    .entry(part.table.clone())
                    .or_insert(json!({"rows":0}));
                let rows = table["rows"]
                    .as_u64()
                    .ok_or("oracle_mixed_rows")?
                    .checked_add(part.rows as u64)
                    .ok_or("oracle_mixed_rows")?;
                *table = json!({"last_sequence":batch.sequence,"rows":rows});
            }
        } else if batch.kind == "cdc" && fills > 0 {
            if batch.previous_position != pos.to_string().as_str()
                || batch.position != (pos + 1).to_string()
            {
                return Err("oracle_mixed_cursor");
            }
            let pin: ArtifactRef =
                serde_json::from_value(txs.get(pos).ok_or("oracle_mixed_transaction")?.clone())
                    .map_err(|_| "oracle_mixed_transaction")?;
            let payload = pin.document()?;
            let expected: Batch = serde_json::from_value(payload["batch"].clone())
                .map_err(|_| "oracle_mixed_transaction")?;
            let position: Position = serde_json::from_value(payload["source_position"].clone())
                .map_err(|_| "oracle_mixed_transaction")?;
            if payload["protocol"] != "oo.oracle.buffer-consumer-input/v1"
                || payload["source_id"] != source
                || payload["buffer_context"] != idx["context"]
                || expected.digest()? != batch.digest()?
                || serde_json::to_value(&position.cursor_before)
                    .map_err(|_| "oracle_mixed_cursor")?
                    != cursor
            {
                return Err("oracle_mixed_transaction");
            }
            bound(reference, source, &batch, Some(&position))?;
            entry["source_evidence_sha256"] = json!(value_hash(&payload["source_position"])?);
            cursor =
                serde_json::to_value(&position.cursor_after).map_err(|_| "oracle_mixed_cursor")?;
            pos += 1;
        } else {
            return Err("oracle_mixed_kind");
        }
        batches.push(entry);
    }
    let snapshot: ArtifactRef = serde_json::from_value(doc["boundary"]["native_snapshot"].clone())
        .map_err(|_| "oracle_mixed_snapshot")?;
    let snapshot = snapshot.document()?;
    let rows = table_plan
        .values()
        .try_fold(0u64, |a, t| a.checked_add(t["rows"].as_u64().unwrap_or(0)))
        .ok_or("oracle_mixed_rows")?;
    if fills == 0
        || pos == 0
        || pos != txs.len()
        || idx["transaction_count"].as_u64() != Some(pos as u64)
        || idx["final_cursor"] != cursor
        || doc["boundary"]["final_position"] != pos.to_string().as_str()
        || doc["batch_count"].as_u64() != Some(batches.len() as u64)
        || snapshot["complete"] != true
        || snapshot["rows"].as_u64() != Some(rows)
        || table_plan.len() != specs.len()
        || table_plan.keys().any(|k| !specs.contains_key(k))
    {
        return Err("oracle_mixed_coverage");
    }
    let plan = json!({"protocol":"oo.oracle.initial-mixed/v1","source_id":source,"config_digest":config_digest,
        "typed_schema_sha256":ctx["typed_schema_sha256"],"buffer_index":index,"buffer_context":reference,
        "bundle_sha256":capture.sha256,"batch_count":batches.len(),"fill_batch_count":fills,
        "final_position":pos.to_string(),"batches":batches,"tables":table_plan});
    if *supplied_plan != plan {
        return Err("oracle_mixed_plan");
    }
    binding["buffer_index"] = json!(index);
    binding["initial_fill"] = plan;
    Ok(binding)
}

/// The descriptor supplies immutable paths; SQL independently verifies its
/// parent receipt against the actual confirmed target before handing off a lease.
pub fn delta(reference: &ArtifactRef, source: &str, previous: &Value) -> Result<Value> {
    let d = reference.document()?;
    let read = |value: &Value| -> Result<Value> {
        let pin: ArtifactRef =
            serde_json::from_value(value.clone()).map_err(|_| "oracle_rollover_pin")?;
        pin.document()
    };
    let parent = read(&d["parent_index"])?;
    let parent_context = read(&parent["context"])?;
    let ack = read(&d["parent_ack"])?;
    let receipt = &ack["receipt"];
    let buffer = &receipt["oracle_buffer"];
    let generation = previous["source_generation"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or("oracle_rollover_parent")?;
    let target_generation = previous["target_generation"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or("oracle_rollover_parent")?;
    let cursor: Cursor = serde_json::from_value(previous["buffer_cursor"].clone())
        .map_err(|_| "oracle_rollover_parent")?;
    let position = cursor.ordinal()?;
    if d["protocol"] != "oo.oracle.buffer-rollover/v1"
        || d["engine"] != "oracle"
        || d["source_id"] != source
        || d["source_generation"].as_i64() != generation.checked_add(1)
        || d["parent_source_generation"] != generation
        || d["previous"] != *previous
        || previous["protocol"] != "oo.jobs.oracle-buffer-previous/v1"
        || previous["engine"] != "oracle"
        || previous["source_id"] != source
        || previous["buffer_index"] != d["parent_index"]
        || previous["buffer_context"] != parent["context"]
        || previous["parent_ack"] != d["parent_ack"]
        || previous["buffer_cursor"] != parent["final_cursor"]
        || d["initial_cursor"] != previous["buffer_cursor"]
        || ack["protocol"] != "oo.oracle.c1-ack-state/v1"
        || ack["acknowledged"] != true
        || ack["task_state"] != "succeeded"
        || ack["request_digest"] != previous["request_digest"]
        || receipt["source_id"] != source
        || receipt["source_generation"] != generation
        || receipt["target_generation"] != target_generation
        || ack["source_generation"] != generation
        || ack["target_generation"] != target_generation
        || ack["applied_pos"] != position.to_string().as_str()
        || receipt["source_epoch"] != previous["source_epoch"]
        || receipt["source_instance"]["lease_id"] != previous["task_id"]
        || receipt["l_end"] != previous["l_end"]
        || receipt["l_end"]
            != format!(
                "oracle-buffer:{}:{position}",
                parent["context"]["sha256"]
                    .as_str()
                    .ok_or("oracle_rollover_parent")?
            )
        || buffer["context_sha256"] != parent["context"]["sha256"]
        || buffer["bundle_sha256"] != parent_context["bundle"]["sha256"]
        || buffer["bundle_sha256"] != previous["bundle_sha256"]
        || buffer["final_cursor"] != previous["buffer_cursor"]
        || buffer["source_identity"] != parent_context["source_identity"]
        || buffer["source_offset_write"] != false
        || ["measured_id", "config_digest", "schema_digest"]
            .iter()
            .any(|k| receipt["source_instance"][*k] != previous[*k])
    {
        return Err("oracle_rollover_parent");
    }
    Ok(d)
}

pub fn open_delta(
    reference: &ArtifactRef,
    source: &str,
    tables: &Value,
    first: &Batch,
    rollover: &ArtifactRef,
    previous: &Value,
) -> Result<Value> {
    let descriptor = delta(rollover, source, previous)?;
    let (ctx, capture) = context(reference, source, true)?;
    let document = capture.document()?;
    if ctx.rollover.as_ref() != Some(rollover)
        || document["rollover"]
            != serde_json::to_value(rollover).map_err(|_| "oracle_rollover_pin")?
        || document["boundary"]["stream_mode"] != "delta"
        || document["typed_tables"] != *tables
        || ctx.typed_schema_sha256 != previous["schema_digest"]
        || serde_json::to_value(&ctx.initial_cursor).map_err(|_| "oracle_rollover_context")?
            != previous["buffer_cursor"]
        || descriptor["source_resume"]["source_offset_write"] != false
        || descriptor["source_resume"]["successor_final_offset"]
            != serde_json::to_value(&ctx.source_resume.final_connector_offset)
                .map_err(|_| "oracle_rollover_pin")?
        || descriptor["source_resume"]["connector_history"]
            != serde_json::to_value(&ctx.source_resume.connector_history)
                .map_err(|_| "oracle_rollover_pin")?
        || first.kind != "cdc"
        || first.sequence != 1
        || first.previous_position != ctx.initial_cursor.transaction_ordinal
    {
        return Err("oracle_rollover_context");
    }
    let mut bound = binding(reference, &ctx, capture.bound(source, first)?);
    bound["rollover_sha256"] = json!(rollover.sha256);
    bound["parent_request_digest"] = previous["request_digest"].clone();
    Ok(bound)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub protocol: String,
    pub engine: String,
    pub checkpoint_kind: String,
    pub context_sha256: String,
    pub bundle_sha256: String,
    pub spool_sha256: String,
    pub cursor_before: Cursor,
    pub cursor_after: Cursor,
    pub transaction_id: String,
    pub commit_scn: String,
    pub event_count: u64,
    pub end_event_count: u64,
    pub event_order_first: u64,
    pub event_order_last: u64,
    pub events_sha256: String,
    pub captured_event_ids: ArtifactRef,
    pub end_evidence: ArtifactRef,
    pub typed_batch_digest: String,
    pub target_apply_ordinal: String,
    pub source_offset_write: bool,
}

impl Position {
    fn validate(&self, batch: &Batch) -> Result<()> {
        let before = self.cursor_before.ordinal()?;
        let after = self.cursor_after.ordinal()?;
        if self.protocol != POSITION
            || self.engine != "oracle"
            || self.checkpoint_kind != "buffer_consumption"
            || self.source_offset_write
            || !scn(&self.commit_scn)
            || !token(&self.transaction_id)
            || [
                &self.context_sha256,
                &self.bundle_sha256,
                &self.spool_sha256,
                &self.events_sha256,
            ]
            .into_iter()
            .any(|s| !hex64(s))
            || before.checked_add(1) != Some(after)
            || self.cursor_after.transaction_id.as_ref() != Some(&self.transaction_id)
            || batch.kind != "cdc"
            || batch.previous_position != before.to_string()
            || batch.position != after.to_string()
            || self.target_apply_ordinal != batch.position
            || self.typed_batch_digest != batch.digest()?
            || self.event_count == 0
            || self.end_event_count != self.event_count
            || self.event_order_first != 1
            || self.event_order_last != self.event_count
            || batch
                .parts
                .iter()
                .try_fold(0u64, |n, p| n.checked_add(p.rows as u64))
                != Some(self.event_count)
        {
            return Err("buffer_transition");
        }
        Ok(())
    }

    fn evidence(&self, batch: &Batch) -> Result<()> {
        let end = self.end_evidence.document()?;
        if end["status"] != "END"
            || end["id"] != self.transaction_id
            || end["event_count"].as_u64() != Some(self.event_count)
        {
            return Err("buffer_end");
        }
        let mut expected = BTreeMap::new();
        for table in end["data_collections"].as_array().ok_or("buffer_end")? {
            let name = table["data_collection"].as_str().ok_or("buffer_end")?;
            let count = table["event_count"]
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or("buffer_end")?;
            if expected.insert(name.to_owned(), count).is_some() {
                return Err("buffer_end");
            }
        }
        let mut actual = BTreeMap::<String, u64>::new();
        let mut target_counts = BTreeMap::<String, u64>::new();
        let mut count = 0u64;
        self.captured_event_ids.json_lines(|event| {
            count = count.checked_add(1).ok_or("buffer_events")?;
            let id = &event["captured_id"];
            if event["protocol"] != "oo.oracle.captured-event-id/v1"
                || !event["event_body_sha256"].as_str().is_some_and(hex64)
                || id["transaction_id"] != self.transaction_id
                || id["total_order"].as_u64() != Some(count)
                || id["commit_scn"] != self.commit_scn
                || !id["scn"].as_str().is_some_and(scn)
                || id["rs_id"].as_str().is_none_or(|s| s.is_empty())
                || id["ssn"].as_u64().is_none()
            {
                return Err("buffer_events");
            }
            let parts = ["db", "schema", "table"].map(|key| {
                id[key]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or("buffer_events")
            });
            let [db, schema, table] = parts;
            let (db, schema, table) = (db?, schema?, table?);
            let actual = actual.entry(format!("{db}.{schema}.{table}")).or_default();
            *actual = actual.checked_add(1).ok_or("buffer_events")?;
            let target = target_counts
                .entry(format!("{schema}.{table}"))
                .or_default();
            *target = target.checked_add(1).ok_or("buffer_events")?;
            Ok(())
        })?;
        if count != self.event_count || actual != expected {
            return Err("buffer_events");
        }
        if batch.parts.len() != target_counts.len()
            || batch
                .parts
                .iter()
                .any(|part| target_counts.get(&part.table) != Some(&(part.rows as u64)))
        {
            return Err("buffer_table_event_count");
        }
        Ok(())
    }
}

pub fn bound(
    reference: &ArtifactRef,
    source: &str,
    batch: &Batch,
    position: Option<&Position>,
) -> Result<Value> {
    let (ctx, capture) = context(reference, source, false)?;
    if ctx.rollover.is_some() && batch.kind != "cdc" {
        return Err("oracle_delta_fill_refused");
    }
    let doc = capture.document()?;
    let mut bound = binding(reference, &ctx, capture.bound(source, batch)?);
    if batch.kind == "fill" {
        if position.is_some() {
            return Err("buffer_snapshot_position");
        }
        return Ok(bound);
    }
    let p = position.ok_or("buffer_position_required")?;
    p.validate(batch)?;
    if p.context_sha256 != reference.sha256
        || p.bundle_sha256 != ctx.bundle.sha256
        || p.spool_sha256 != ctx.raw_spool.sha256
        || bound["checkpoint"]["transaction_id"] != p.transaction_id
        || bound["checkpoint"]["commit_scn"] != p.commit_scn
        || bound["checkpoint"]["event_sha256"] != p.events_sha256
    {
        return Err("buffer_position_binding");
    }
    let previous = p.cursor_before.ordinal()?;
    if previous > 0
        && doc["checkpoints"][previous as usize - 1]["transaction_id"]
            != json!(p.cursor_before.transaction_id)
    {
        return Err("buffer_previous_transaction");
    }
    p.evidence(batch)?;
    let mut evidence = serde_json::to_value(p).map_err(|_| "buffer_position")?;
    // Match the producer's replay_digest: canonical complete input position,
    // including its pinned artifact refs. Stored evidence omits local paths.
    bound["source_evidence_sha256"] = json!(value_hash(&evidence)?);
    for key in ["captured_event_ids", "end_evidence"] {
        evidence[key]
            .as_object_mut()
            .ok_or("buffer_position")?
            .remove("path");
    }
    bound["buffer_position"] = evidence;
    Ok(bound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires pinned owned mixed input and plan; no source/target sessions"]
    fn retained_mixed_initial_plan_distinguishes_fill_sequence_from_cdc_cursor() {
        let cfg: Value = serde_json::from_slice(
            &std::fs::read(std::env::var("OO_ORACLE_MIXED_CONFIG").unwrap()).unwrap(),
        )
        .unwrap();
        let plan: Value = serde_json::from_slice(
            &std::fs::read(std::env::var("OO_ORACLE_MIXED_PLAN").unwrap()).unwrap(),
        )
        .unwrap();
        let index: ArtifactRef = serde_json::from_value(cfg["buffer_index"].clone()).unwrap();
        let idx = index.document().unwrap();
        let ctx: ArtifactRef = serde_json::from_value(idx["context"].clone()).unwrap();
        let capture: ArtifactRef = serde_json::from_value(cfg["bundle"].clone()).unwrap();
        let doc = capture.document().unwrap();
        let raw = std::fs::read(capture.path.parent().unwrap().join("batches.jsonl")).unwrap();
        let first: Value =
            serde_json::from_slice(raw.split(|b| *b == b'\n').next().unwrap()).unwrap();
        let batch: Batch = serde_json::from_value(first["batch"].clone()).unwrap();
        let check = |v: &Value| {
            open_initial_fill(
                &ctx,
                &index,
                "oracle_initial_resume_20261028",
                &doc["typed_tables"],
                &batch,
                v,
                plan["config_digest"].as_str().unwrap(),
            )
        };
        let binding = check(&plan).unwrap();
        assert_eq!(binding["initial_fill"], plan);
        assert_eq!(plan["batches"][39]["position"], "0");
        assert_eq!(plan["batches"][40]["position"], "1");
        for (pointer, value) in [
            ("/protocol", json!("oo.oracle.initial-fill/v1")),
            ("/source_id", json!("wrong")),
            ("/fill_batch_count", json!(41)),
            ("/final_position", json!("41")),
            ("/batches/40/position", json!("41")),
            ("/batches/40/source_evidence_sha256", json!("0".repeat(64))),
            ("/tables/OO_SNAP28_01.SNAPSHOT_ROWS/rows", json!(10001)),
        ] {
            let mut bad = plan.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(check(&bad).is_err(), "{pointer}");
        }
    }

    #[test]
    #[ignore = "requires immutable owned initial-fill OPEN frame; no source or target connection"]
    fn retained_initial_fill_packet_recomputes_plan_and_rejects_tampering() {
        let bytes = std::fs::read(std::env::var("OO_ORACLE_FILL_TEST_OPEN").unwrap()).unwrap();
        let frame: Value = serde_json::from_slice(&bytes).unwrap();
        let context: ArtifactRef = serde_json::from_value(frame["buffer_context"].clone()).unwrap();
        let index: ArtifactRef = serde_json::from_value(frame["buffer_index"].clone()).unwrap();
        let doc = context.document().unwrap();
        let capture: ArtifactRef = serde_json::from_value(doc["bundle"].clone()).unwrap();
        let raw = std::fs::read(capture.path.parent().unwrap().join("batches.jsonl")).unwrap();
        let first: Value =
            serde_json::from_slice(raw.split(|b| *b == b'\n').next().unwrap()).unwrap();
        let batch: Batch = serde_json::from_value(first["batch"].clone()).unwrap();
        let check = |plan: &Value| {
            open_initial_fill(
                &context,
                &index,
                doc["source_id"].as_str().unwrap(),
                &frame["typed_tables"],
                &batch,
                plan,
                frame["config_digest"].as_str().unwrap(),
            )
        };
        let bound = check(&frame["initial_fill"]).unwrap();
        assert_eq!(bound["initial_fill"], frame["initial_fill"]);
        for (pointer, value) in [
            ("/batch_count", json!(0)),
            ("/source_id", json!("wrong_source")),
            ("/batches/0/digest", json!("0".repeat(64))),
            ("/buffer_context/sha256", json!("0".repeat(64))),
        ] {
            let mut wrong = frame["initial_fill"].clone();
            *wrong.pointer_mut(pointer).unwrap() = value;
            assert!(check(&wrong).is_err(), "{pointer}");
        }
    }

    #[test]
    fn initial_fill_plan_uses_exact_batch_images_and_rejects_gaps_cdc_and_wrong_coverage() {
        let (_, mut batch) = fixture();
        batch.sequence = 1;
        batch.kind = "fill".into();
        batch.position = "0".into();
        let document =
            json!({"typed_tables":{"S.T":batch.parts[0].spec},"batch_count":1,"final_rows":2});
        let raw = |b: &Batch| serde_json::to_vec(&json!({"kind":"typed_batch","batch":b})).unwrap();
        let (batches, tables) = initial_fill_batches(&document, &raw(&batch)).unwrap();
        assert_eq!(batches[0]["digest"], batch.digest().unwrap());
        assert_eq!(tables, json!({"S.T":{"last_sequence":1,"rows":2}}));
        for (sequence, kind, position) in [(2, "fill", "0"), (1, "cdc", "1"), (1, "fill", "1")] {
            let mut wrong = batch.clone();
            wrong.sequence = sequence;
            wrong.kind = kind.into();
            wrong.position = position.into();
            assert!(initial_fill_batches(&document, &raw(&wrong)).is_err());
        }
        let mut wrong = document.clone();
        wrong["final_rows"] = json!(3);
        assert!(initial_fill_batches(&wrong, &raw(&batch)).is_err());
        wrong = document.clone();
        wrong["typed_tables"]["S.T"]["keys"] = json!([]);
        assert!(initial_fill_batches(&wrong, &raw(&batch)).is_err());
    }

    #[test]
    #[ignore = "requires the fixed owned Oracle delta input packet; no source connection"]
    fn real_delta_packet_matches_parent_and_rejects_cross_source() {
        let path = std::env::var("OO_ORACLE_DELTA_TEST_INPUT").unwrap();
        let packet: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let source = "oracle_frozen_iud_20261027";
        let previous = &packet["actual_previous"];
        let rollover: ArtifactRef = serde_json::from_value(packet["rollover"].clone()).unwrap();
        delta(&rollover, source, previous).unwrap();
        let index: ArtifactRef = serde_json::from_value(packet["buffer_index"].clone()).unwrap();
        let index = index.document().unwrap();
        let context: ArtifactRef = serde_json::from_value(index["context"].clone()).unwrap();
        let context_doc = context.document().unwrap();
        let bundle: ArtifactRef = serde_json::from_value(context_doc["bundle"].clone()).unwrap();
        let bundle_doc = bundle.document().unwrap();
        let payload: ArtifactRef =
            serde_json::from_value(index["transactions"][0].clone()).unwrap();
        let batch: Batch =
            serde_json::from_value(payload.document().unwrap()["batch"].clone()).unwrap();
        let bound = open_delta(
            &context,
            source,
            &bundle_doc["typed_tables"],
            &batch,
            &rollover,
            previous,
        )
        .unwrap();
        assert_eq!(bound["rollover_sha256"], rollover.sha256);
        assert_eq!(bound["parent_request_digest"], previous["request_digest"]);
        assert!(open(&context, source, &bundle_doc["typed_tables"], &batch).is_err());
        assert!(delta(&rollover, "other_source", previous).is_err());
        let mut wrong = previous.clone();
        wrong["source_generation"] = json!(2);
        assert!(delta(&rollover, source, &wrong).is_err());
    }

    #[test]
    #[ignore = "requires a retained captured index; offline target admission only"]
    fn retained_capture_batches_bind_complete_table_partitions() {
        let path = std::env::var("OO_ORACLE_CAPTURE_INDEX").unwrap();
        let index: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let reference: ArtifactRef = serde_json::from_value(index["context"].clone()).unwrap();
        let context = reference.document().unwrap();
        let source = context["source_id"].as_str().unwrap();
        let entries = index["transactions"].as_array().unwrap();
        assert!(
            !entries.is_empty(),
            "this test must exercise CDC evidence, not only a snapshot"
        );
        for entry in entries {
            let entry: ArtifactRef = serde_json::from_value(entry.clone()).unwrap();
            let payload = entry.document().unwrap();
            let batch: Batch = serde_json::from_value(payload["batch"].clone()).unwrap();
            let position: Position =
                serde_json::from_value(payload["source_position"].clone()).unwrap();
            batch.validate().unwrap();
            bound(&reference, source, &batch, Some(&position)).unwrap();
            assert!(bound(&reference, "different_source", &batch, Some(&position)).is_err());
        }
    }

    #[test]
    fn final_scope_keeps_capture_ordinal_and_measured_scn_separate() {
        let identity = json!({"engine":"oracle","dbid":"42","resetlogs_change":"100","pdb_guid":"A".repeat(32),
            "pdb_name":"P","db_unique_name":"D"});
        let lineage = format!("oracle.42.100.{}", "a".repeat(32));
        let mut scope = json!({"protocol":SCOPE,"engine":"oracle","lineage":lineage,"source_identity":{"engine":"oracle","measured_id":lineage},
            "schema_digest":"d".repeat(64),"complete":true,"consistency_grade":"T","cross_table_consistent":true,
            "l0":format!("oracle-buffer:{}:0","a".repeat(64)),"l_end":format!("oracle-buffer:{}:4","a".repeat(64)),
            "buffer":{"checkpoint_kind":"buffer_consumption","context_sha256":"a".repeat(64),"bundle_sha256":"b".repeat(64),
                "spool_sha256":"c".repeat(64),"typed_schema_sha256":"d".repeat(64),"connector_offsets_sha256":"e".repeat(64),
                "connector_history_sha256":"f".repeat(64),"source_identity":identity,"source_offset_write":false,
                "final_cursor":{"transaction_ordinal":"4","transaction_id":"txn"},"commit_scn":"2133689"}});
        validate_scope(&scope).unwrap();
        scope["l0"] = json!(format!("oracle-buffer:{}:3", "a".repeat(64)));
        validate_scope(&scope).unwrap();
        for start in ["04", "4", "5", "-1"] {
            scope["l0"] = json!(format!("oracle-buffer:{}:{start}", "a".repeat(64)));
            assert!(validate_scope(&scope).is_err(), "{start}");
        }
        scope["l0"] = json!(format!("oracle-buffer:{}:0", "a".repeat(64)));
        scope["buffer"]["commit_scn"] = json!("0");
        assert!(validate_scope(&scope).is_err());
        scope["buffer"]["commit_scn"] = json!("2133689");
        scope["engine"] = json!("mysql");
        assert!(validate_scope(&scope).is_err());
        assert!(crate::native_binding::approved_admission_bin(
            crate::native_binding::ADMISSION_BIN_SHA256_ORACLE_BUFFER_20261027
        ));
        assert!(crate::native_binding::approved_admission_bin(
            crate::native_binding::ADMISSION_BIN_SHA256_MACOS_C1_20261027
        ));
        assert!(!crate::native_binding::approved_admission_bin(
            &"f".repeat(64)
        ));
    }

    fn fixture() -> (Position, Batch) {
        let batch: Batch = serde_json::from_value(json!({
            "protocol":"oo.c1.typed-batch/v3","sequence":2,"kind":"cdc","previous_position":"0","position":"1",
            "parts":[{"table":"S.T","path":"/unit/part.arrow","sha256":"a".repeat(64),"bytes":10,"rows":2,
                "spec":{"columns":[{"name":"ID","oid":20,"typmod":-1,"nullable":false}],"keys":[0]}}]
        })).unwrap();
        let position: Position = serde_json::from_value(json!({
            "protocol":POSITION,"engine":"oracle","checkpoint_kind":"buffer_consumption",
            "context_sha256":"a".repeat(64),"bundle_sha256":"b".repeat(64),"spool_sha256":"c".repeat(64),
            "cursor_before":{"transaction_ordinal":"0","transaction_id":null},
            "cursor_after":{"transaction_ordinal":"1","transaction_id":"tx1"},"transaction_id":"tx1",
            "commit_scn":"2126801","event_count":2,"end_event_count":2,"event_order_first":1,"event_order_last":2,
            "events_sha256":"d".repeat(64),"typed_batch_digest":batch.digest().unwrap(),"target_apply_ordinal":"1",
            "source_offset_write":false,
            "captured_event_ids":{"path":"/unit/events","sha256":"e".repeat(64),"bytes":1},
            "end_evidence":{"path":"/unit/end","sha256":"f".repeat(64),"bytes":1}
        })).unwrap();
        (position, batch)
    }

    #[test]
    fn buffer_requires_complete_contiguous_transaction_not_vendor_offsets() {
        let (p, batch) = fixture();
        p.validate(&batch).unwrap();
        for key in ["offset_before", "offset_after", "redo_thread"] {
            let mut value = serde_json::to_value(&p).unwrap();
            value[key] = json!(0);
            assert!(serde_json::from_value::<Position>(value).is_err());
        }
        for (key, value) in [
            ("protocol", json!("oo.c1.oracle-logminer-position/v1")),
            ("source_offset_write", json!(true)),
            ("end_event_count", json!(1)),
            ("event_order_first", json!(0)),
            ("commit_scn", json!("0")),
            ("typed_batch_digest", json!("0".repeat(64))),
            ("target_apply_ordinal", json!("2126801")),
            (
                "cursor_after",
                json!({"transaction_ordinal":"2","transaction_id":"tx1"}),
            ),
        ] {
            let mut bad = serde_json::to_value(&p).unwrap();
            bad[key] = value;
            assert!(
                serde_json::from_value::<Position>(bad)
                    .unwrap()
                    .validate(&batch)
                    .is_err(),
                "{key}"
            );
        }
    }

    #[test]
    fn unknown_cursor_identity_is_not_zero_and_scn_is_not_a_sequence() {
        assert!(
            serde_json::from_value::<Cursor>(
                json!({"transaction_ordinal":"1","transaction_id":null})
            )
            .unwrap()
            .ordinal()
            .is_err()
        );
        let (mut p, mut batch) = fixture();
        p.validate(&batch).unwrap();
        batch.sequence += 1;
        batch.previous_position = "1".into();
        batch.position = "2".into();
        p.cursor_before = p.cursor_after;
        p.cursor_after = Cursor {
            transaction_ordinal: "2".into(),
            transaction_id: Some("tx2".into()),
        };
        p.transaction_id = "tx2".into();
        p.target_apply_ordinal = "2".into();
        p.typed_batch_digest = batch.digest().unwrap();
        p.validate(&batch).unwrap(); // Same measured SCN, different complete transaction.
        p.commit_scn = "90000000000000000000000000000000000000".into();
        p.validate(&batch).unwrap();
        assert_eq!(
            Cursor {
                transaction_ordinal: "1000001".into(),
                transaction_id: Some("tx".into())
            }
            .ordinal()
            .unwrap(),
            1_000_001
        );
        p.cursor_before.transaction_ordinal = u64::MAX.to_string();
        assert!(
            p.validate(&batch).is_err(),
            "overflow must refuse, not wrap or panic"
        );
    }

    #[test]
    fn complete_101k_evidence_streams_past_32_mib_and_rejects_bad_last_event() {
        use std::io::{Seek, SeekFrom, Write};
        let root = std::env::temp_dir().join(format!("oo-buffer-large-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let (mut p, mut batch) = fixture();
        let count = 101000u64;
        batch.parts[0].rows = 50500;
        let mut second = batch.parts[0].clone();
        second.table = "S.U".into();
        batch.parts.push(second);
        p.event_count = count;
        p.end_event_count = count;
        p.event_order_last = count;
        p.typed_batch_digest = batch.digest().unwrap();
        let end = serde_json::to_vec(&json!({"status":"END","id":"tx1","event_count":count,
            "data_collections":[{"data_collection":"DB.S.T","event_count":50500},
                                {"data_collection":"DB.S.U","event_count":50500}]}))
        .unwrap();
        let end_path = root.join("end");
        std::fs::write(&end_path, &end).unwrap();
        p.end_evidence = ArtifactRef {
            path: end_path,
            sha256: sha256_hex(&end),
            bytes: end.len() as u64,
        };
        let path = root.join("events");
        let mut file = File::create(&path).unwrap();
        let mut hash = Sha256::new();
        let (mut length, mut last_offset) = (0u64, 0u64);
        for order in 1..=count {
            let table = if order <= 50500 { "T" } else { "U" };
            let raw=format!("{}\n",json!({"protocol":"oo.oracle.captured-event-id/v1",
                "event_body_sha256":"a".repeat(64),"captured_id":{
                    "transaction_id":"tx1","total_order":order,"commit_scn":"2126801","scn":"2126800",
                    "rs_id":"r".repeat(128),"ssn":0,"db":"DB","schema":"S","table":table}})).into_bytes();
            last_offset = length;
            length += raw.len() as u64;
            file.write_all(&raw).unwrap();
            hash.update(&raw);
        }
        drop(file);
        assert!(length > EVIDENCE);
        p.captured_event_ids = ArtifactRef {
            path: path.clone(),
            bytes: length,
            sha256: format!("{:x}", hash.finalize()),
        };
        p.validate(&batch).unwrap();
        p.evidence(&batch).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.seek(SeekFrom::Start(last_offset)).unwrap();
        let mut raw = Vec::new();
        file.read_to_end(&mut raw).unwrap();
        let altered = String::from_utf8(raw)
            .unwrap()
            .replace("\"total_order\":101000", "\"total_order\":100999");
        file.seek(SeekFrom::Start(last_offset)).unwrap();
        file.write_all(altered.as_bytes()).unwrap();
        drop(file);
        assert_eq!(p.evidence(&batch).unwrap_err(), "buffer_artifact");
        let mut file = File::open(&path).unwrap();
        let mut hash = Sha256::new();
        let mut chunk = [0u8; 65536];
        loop {
            let n = file.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            hash.update(&chunk[..n]);
        }
        p.captured_event_ids.sha256 = format!("{:x}", hash.finalize());
        assert_eq!(p.evidence(&batch).unwrap_err(), "buffer_events");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artifact_checks_exact_hash_size_and_path_without_interpreting_vendor_bytes() {
        let path = std::env::temp_dir().join(format!("oo-buffer-artifact-{}", std::process::id()));
        std::fs::write(&path, b"captured").unwrap();
        let r = ArtifactRef {
            path: path.clone(),
            sha256: sha256_hex(b"captured"),
            bytes: 8,
        };
        r.verify(8).unwrap();
        assert!(r.verify(7).is_err());
        std::fs::write(&path, b"tampered").unwrap();
        assert!(r.verify(8).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn end_and_captured_ids_must_be_complete_and_unknown_fields_stay_unknown() {
        let root = std::env::temp_dir().join(format!("oo-buffer-evidence-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let path = root.join(name);
            std::fs::write(&path, bytes).unwrap();
            ArtifactRef {
                path,
                bytes: bytes.len() as u64,
                sha256: sha256_hex(bytes),
            }
        };
        let (mut p, _) = fixture();
        p.end_evidence = write(
            "end",
            &serde_json::to_vec(&json!({"status":"END","id":"tx1","event_count":2,
            "data_collections":[{"data_collection":"DB.S.T","event_count":2}]}))
            .unwrap(),
        );
        let events=(1..=2).map(|order| json!({"protocol":"oo.oracle.captured-event-id/v1",
            "event_body_sha256":"a".repeat(64),"captured_id":{"transaction_id":"tx1","total_order":order,
                "commit_scn":"2126801","scn":"2126800","rs_id":"measured-record","ssn":0,
                "db":"DB","schema":"S","table":"T"}})).collect::<Vec<_>>();
        let encode = |rows: &[Value]| {
            rows.iter()
                .map(|v| format!("{v}\n"))
                .collect::<String>()
                .into_bytes()
        };
        p.captured_event_ids = write("events", &encode(&events));
        let (_, batch) = fixture();
        p.evidence(&batch).unwrap();
        let mut wrong_table = batch.clone();
        wrong_table.parts[0].table = "S.OTHER".into();
        assert_eq!(
            p.evidence(&wrong_table).unwrap_err(),
            "buffer_table_event_count"
        );
        let mut bad = events.clone();
        bad[0]["captured_id"].as_object_mut().unwrap().remove("ssn");
        p.captured_event_ids = write("missing-ssn", &encode(&bad));
        assert_eq!(p.evidence(&batch).unwrap_err(), "buffer_events");
        bad = events.clone();
        bad[1]["captured_id"]["total_order"] = json!(1);
        p.captured_event_ids = write("duplicate-order", &encode(&bad));
        assert!(p.evidence(&batch).is_err());
        p.captured_event_ids = write("truncated", &encode(&events[..1]));
        assert!(p.evidence(&batch).is_err());
        p.captured_event_ids = write("events", &encode(&events));
        p.end_evidence = write(
            "wrong-end",
            br#"{"status":"END","id":"other_tx","event_count":2}"#,
        );
        assert_eq!(p.evidence(&batch).unwrap_err(), "buffer_end");
        let mut multi = batch.clone();
        multi.parts[0].rows = 1;
        let mut second = multi.parts[0].clone();
        second.table = "S.U".into();
        multi.parts.push(second);
        let mut multi_events = events.clone();
        multi_events[1]["captured_id"]["table"] = json!("U");
        p.captured_event_ids = write("multi-events", &encode(&multi_events));
        p.end_evidence = write(
            "multi-end",
            &serde_json::to_vec(&json!({
                "status":"END","id":"tx1","event_count":2,"data_collections":[
                    {"data_collection":"DB.S.T","event_count":1},
                    {"data_collection":"DB.S.U","event_count":1}]
            }))
            .unwrap(),
        );
        p.typed_batch_digest = multi.digest().unwrap();
        p.validate(&multi).unwrap();
        p.evidence(&multi).unwrap();
        multi.parts[0].rows = 2;
        multi.parts[1].rows = 0;
        assert_eq!(p.evidence(&multi).unwrap_err(), "buffer_table_event_count");
        std::fs::remove_dir_all(root).unwrap();
    }
}
