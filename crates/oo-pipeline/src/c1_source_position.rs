//! Source checkpoints are not the target apply ordinal. In particular Oracle
//! SCNs are neither contiguous nor unique transaction IDs.
use crate::target_installer::sha256_hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};
#[path = "c1_oracle_source_resume.rs"]
mod source_resume;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "engine", rename_all = "lowercase", deny_unknown_fields)]
pub enum Identity {
    Mysql {
        server_uuid: String,
    },
    Oracle {
        dbid: String,
        resetlogs_change: String,
        pdb_guid: String,
        pdb_name: String,
        db_unique_name: String,
    },
}
fn decimal(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 38
        && s.bytes().all(|b| b.is_ascii_digit())
        && (s.len() == 1 || !s.starts_with('0'))
}
fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'$' || b == b'#')
}
pub fn hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Identity {
    pub fn engine(&self) -> &'static str {
        match self {
            Self::Mysql { .. } => "mysql",
            Self::Oracle { .. } => "oracle",
        }
    }
    pub fn measured_id(&self) -> Result<String, &'static str> {
        match self {
            Self::Mysql { server_uuid } => {
                if server_uuid.len() != 36
                    || !server_uuid.bytes().enumerate().all(|(i, b)| {
                        if [8, 13, 18, 23].contains(&i) {
                            b == b'-'
                        } else {
                            b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
                        }
                    })
                {
                    return Err("source_identity");
                }
                Ok(format!("mysql.{server_uuid}"))
            }
            Self::Oracle {
                dbid,
                resetlogs_change,
                pdb_guid,
                pdb_name,
                db_unique_name,
            } => {
                if !decimal(dbid)
                    || dbid == "0"
                    || !decimal(resetlogs_change)
                    || pdb_guid.len() != 32
                    || !pdb_guid
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
                    || !token(pdb_name)
                    || !token(db_unique_name)
                {
                    return Err("source_identity");
                }
                Ok(format!(
                    "oracle.{dbid}.{resetlogs_change}.{}",
                    pdb_guid.to_ascii_lowercase()
                ))
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpaqueOffset {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}
impl OpaqueOffset {
    pub fn verify(&self) -> Result<(), &'static str> {
        if !self.path.is_absolute()
            || self
                .path
                .components()
                .any(|p| p == std::path::Component::ParentDir)
            || !hex64(&self.sha256)
            || self.bytes == 0
            || self.bytes > 1024 * 1024
        {
            return Err("opaque_checkpoint");
        }
        let mut file = File::open(&self.path).map_err(|_| "opaque_checkpoint")?;
        let meta = file.metadata().map_err(|_| "opaque_checkpoint")?;
        if !meta.is_file() || meta.len() != self.bytes {
            return Err("opaque_checkpoint");
        }
        let mut hash = Sha256::new();
        let mut bytes = [0u8; 64 * 1024];
        let mut seen = 0u64;
        loop {
            let n = file.read(&mut bytes).map_err(|_| "opaque_checkpoint")?;
            if n == 0 {
                break;
            }
            seen = seen.checked_add(n as u64).ok_or("opaque_checkpoint")?;
            if seen > self.bytes {
                return Err("opaque_checkpoint");
            }
            hash.update(&bytes[..n]);
        }
        let after = file.metadata().map_err(|_| "opaque_checkpoint")?;
        if seen != self.bytes
            || format!("{:x}", hash.finalize()) != self.sha256
            || after.len() != meta.len()
            || after.modified().ok() != meta.modified().ok()
        {
            return Err("opaque_checkpoint");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePosition {
    pub protocol: String,
    pub engine: String,
    pub commit_scn: String,
    pub transaction_id: String,
    pub event_count: u64,
    pub end_event_count: u64,
    pub event_order_first: u64,
    pub event_order_last: u64,
    pub events_sha256: String,
    pub offset_before: OpaqueOffset,
    pub offset_after: OpaqueOffset,
}
impl OraclePosition {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.protocol != "oo.c1.oracle-logminer-position/v1"
            || self.engine != "oracle"
            || !decimal(&self.commit_scn)
            || self.commit_scn == "0"
            || !token(&self.transaction_id)
            || self.event_count == 0
            || self.end_event_count != self.event_count
            || self.event_order_first != 1
            || self.event_order_last != self.event_count
            || !hex64(&self.events_sha256)
        {
            return Err("oracle_position");
        }
        self.offset_before.verify()?;
        self.offset_after.verify()?;
        if self.offset_before.sha256 == self.offset_after.sha256 {
            return Err("opaque_checkpoint_not_advanced");
        }
        Ok(())
    }
    /// Paths are local transport handles, not identity. Preserve exact opaque
    /// digests without parsing, modifying, or deriving offsets from SCN.
    pub fn bound(&self) -> Result<serde_json::Value, &'static str> {
        self.validate()?;
        let mut v = serde_json::to_value(self).map_err(|_| "oracle_position")?;
        for k in ["offset_before", "offset_after"] {
            v[k].as_object_mut()
                .ok_or("oracle_position")?
                .remove("path");
        }
        Ok(v)
    }
}

/// A durable captured-bundle cursor is NOT a vendor offset. Intermediate
/// transaction ACKs advance only target consumption of this immutable bundle.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturePosition {
    pub protocol: String,
    pub bundle: OpaqueOffset,
}

impl CapturePosition {
    pub fn document(&self) -> Result<serde_json::Value, &'static str> {
        if self.protocol != "oo.c1.oracle-buffer-cursor/v1" {
            return Err("oracle_buffer_cursor");
        }
        self.bundle.verify()?;
        let raw = std::fs::read(&self.bundle.path).map_err(|_| "oracle_bundle")?;
        if raw.len() as u64 != self.bundle.bytes || sha256_hex(&raw) != self.bundle.sha256 {
            return Err("oracle_bundle");
        }
        let doc: serde_json::Value = serde_json::from_slice(&raw).map_err(|_| "oracle_bundle")?;
        if doc["protocol"] != "oo.oracle.producer-bundle/v1"
            || doc["engine"] != "oracle"
            || doc["boundary"]["position_codec"] != "oracle-capture-sequence/v1"
            || !matches!(
                doc["boundary"]["mode"].as_str(),
                Some("debezium_snapshot_and_log" | "oracle_scn_snapshot_and_log")
            )
            || !doc["boundary"]["initial_position"]
                .as_str()
                .is_some_and(|s| {
                    s.parse::<u64>().ok().is_some_and(|n| {
                        n.to_string() == s
                            && (n == 0
                                || doc["boundary"]["stream_mode"] == "delta"
                                    && doc["rollover"].is_object())
                    })
                })
            || !doc["boundary"]["spool_sha256"].as_str().is_some_and(hex64)
            || !doc["boundary"]["connector_offsets_sha256"]
                .as_str()
                .is_some_and(hex64)
            || !doc["boundary"]["connector_history_sha256"]
                .as_str()
                .is_some_and(hex64)
            || !doc["boundary"]["snapshot_scn"]
                .as_str()
                .is_some_and(decimal)
        {
            return Err("oracle_bundle");
        }
        if doc["boundary"]["mode"] == "oracle_scn_snapshot_and_log" {
            source_resume::validate(&doc)?;
        } else if doc["boundary"].get("source_resume_proof").is_some()
            || doc["boundary"].get("native_snapshot").is_some()
            || doc["boundary"]["snapshot_origin"] == "native_as_of_scn"
        {
            return Err("oracle_bundle_mode");
        }
        Ok(doc)
    }

    pub fn identity(&self) -> Result<Identity, &'static str> {
        let doc = self.document()?;
        Self::identity_from_document(&doc)
    }

    fn identity_from_document(doc: &serde_json::Value) -> Result<Identity, &'static str> {
        let parts = doc["boundary"]["identity"]
            .as_str()
            .ok_or("oracle_identity")?
            .split('|')
            .collect::<Vec<_>>();
        if parts.len() != 6 {
            return Err("oracle_identity");
        }
        let id = Identity::Oracle {
            dbid: parts[0].into(),
            resetlogs_change: parts[1].into(),
            pdb_guid: parts[2].into(),
            pdb_name: parts[3].into(),
            db_unique_name: parts[4].into(),
        };
        id.measured_id()?;
        Ok(id)
    }

    pub fn bound(
        &self,
        source: &str,
        batch: &crate::c1_typed::Batch,
    ) -> Result<serde_json::Value, &'static str> {
        let doc = self.document()?;
        if doc["source_id"] != source {
            return Err("oracle_bundle_source");
        }
        batch.validate()?;
        let parent = self.bundle.path.parent().ok_or("oracle_bundle_path")?;
        let expected_sha = doc["artifacts"]["batches.jsonl"]
            .as_str()
            .filter(|s| hex64(s))
            .ok_or("oracle_bundle_batches")?;
        verify_batch_stream(&parent.join("batches.jsonl"), expected_sha, batch)?;
        let checkpoints = doc["checkpoints"]
            .as_array()
            .ok_or("oracle_bundle_checkpoints")?;
        let checkpoint = if batch.kind == "cdc" {
            let position = batch
                .position
                .parse::<usize>()
                .map_err(|_| "oracle_apply_ordinal")?;
            let checkpoint = checkpoints
                .get(position.checked_sub(1).ok_or("oracle_apply_ordinal")?)
                .ok_or("oracle_checkpoint")?;
            if checkpoint["application_position"] != batch.position
                || !checkpoint["commit_scn"].as_str().is_some_and(decimal)
                || !checkpoint["transaction_id"].as_str().is_some_and(token)
                || !checkpoint["event_sha256"].as_str().is_some_and(hex64)
                || checkpoint["event_count"].as_u64()
                    != Some(
                        batch
                            .parts
                            .iter()
                            .try_fold(0u64, |total, p| total.checked_add(p.rows as u64))
                            .ok_or("oracle_checkpoint")?,
                    )
            {
                return Err("oracle_checkpoint");
            }
            checkpoint.clone()
        } else {
            serde_json::Value::Null
        };
        // Reuse this call's verified document; reloading it rehashes the full source proof.
        Ok(
            serde_json::json!({"protocol":"oo.c1.oracle-buffer-cursor/v1","checkpoint_kind":"buffer_consumption",
            "engine":"oracle","source_id":source,"measured_id":Self::identity_from_document(&doc)?.measured_id()?,"bundle_sha256":self.bundle.sha256,
            "spool_sha256":doc["boundary"]["spool_sha256"],"apply_ordinal":batch.position,
            "final_apply_ordinal":doc["boundary"]["final_position"],"snapshot_scn":doc["boundary"]["snapshot_scn"],
            "connector_offsets_sha256":doc["boundary"]["connector_offsets_sha256"],
            "connector_history_sha256":doc["boundary"]["connector_history_sha256"],
            "checkpoint":checkpoint,"source_offset_advanced":false}),
        )
    }
}

fn verify_batch_stream(
    path: &Path,
    expected_sha: &str,
    batch: &crate::c1_typed::Batch,
) -> Result<(), &'static str> {
    // Keep the existing per-frame budget, not a lifetime capture-file ceiling.
    const FRAME_BYTES: u64 = 32 * 1024 * 1024;
    let file = File::open(path).map_err(|_| "oracle_bundle_batches")?;
    let before = file.metadata().map_err(|_| "oracle_bundle_batches")?;
    if !before.is_file() {
        return Err("oracle_bundle_batches");
    }
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hash = Sha256::new();
    let mut bytes = Vec::new();
    let (mut sequence, mut seen, mut found) = (0u64, 0u64, false);
    let expected_digest = batch.digest()?;
    loop {
        bytes.clear();
        let n = reader
            .by_ref()
            .take(FRAME_BYTES + 1)
            .read_until(b'\n', &mut bytes)
            .map_err(|_| "oracle_bundle_batches")?;
        if n == 0 {
            break;
        }
        if n as u64 > FRAME_BYTES || bytes.last() != Some(&b'\n') {
            return Err("oracle_bundle_batches");
        }
        seen = seen.checked_add(n as u64).ok_or("oracle_bundle_batches")?;
        if seen > before.len() {
            return Err("oracle_bundle_batches");
        }
        hash.update(&bytes);
        sequence = sequence.checked_add(1).ok_or("oracle_bundle_batches")?;
        let frame: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "oracle_bundle_batches")?;
        let recorded: crate::c1_typed::Batch =
            serde_json::from_value(frame["batch"].clone()).map_err(|_| "oracle_bundle_batches")?;
        if frame["kind"] != "typed_batch" || recorded.sequence as u64 != sequence {
            return Err("oracle_batch_conflict");
        }
        recorded.validate()?;
        if recorded.sequence == batch.sequence {
            if recorded.digest()? != expected_digest {
                return Err("oracle_batch_conflict");
            }
            found = true;
        }
    }
    let after = reader
        .get_ref()
        .metadata()
        .map_err(|_| "oracle_bundle_batches")?;
    if seen != before.len()
        || after.len() != before.len()
        || after.modified().ok() != before.modified().ok()
        || format!("{:x}", hash.finalize()) != expected_sha
    {
        return Err("oracle_bundle_batches");
    }
    if !found {
        return Err("oracle_bundle_batch_absent");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "oo-oracle-position-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn capture_ordinal_above_one_million_keeps_canonical_rollover_and_wire_bounds() {
        let root = test_root("ordinal");
        let path = root.join("manifest.json");
        let mut document = serde_json::json!({"protocol":"oo.oracle.producer-bundle/v1","engine":"oracle",
            "rollover":{},"boundary":{"position_codec":"oracle-capture-sequence/v1",
                "mode":"debezium_snapshot_and_log","stream_mode":"delta","initial_position":"1000001",
                "spool_sha256":"a".repeat(64),"connector_offsets_sha256":"b".repeat(64),
                "connector_history_sha256":"c".repeat(64),"snapshot_scn":"42"}});
        let check = |document: &serde_json::Value| {
            let raw = serde_json::to_vec(document).unwrap();
            std::fs::write(&path, &raw).unwrap();
            CapturePosition {
                protocol: "oo.c1.oracle-buffer-cursor/v1".into(),
                bundle: OpaqueOffset {
                    path: path.clone(),
                    sha256: sha256_hex(&raw),
                    bytes: raw.len() as u64,
                },
            }
            .document()
        };
        assert!(check(&document).is_ok());
        for invalid in ["01000001", "-1", "18446744073709551616"] {
            document["boundary"]["initial_position"] = serde_json::json!(invalid);
            assert!(check(&document).is_err());
        }
        document["boundary"]["initial_position"] = serde_json::json!("1000001");
        document.as_object_mut().unwrap().remove("rollover");
        assert!(
            check(&document).is_err(),
            "nonzero continuation still requires rollover provenance"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn complete_101k_position_retains_end_order_and_opaque_offset_checks() {
        let root = test_root("events");
        let make = |name: &str, raw: &[u8]| {
            let path = root.join(name);
            std::fs::write(&path, raw).unwrap();
            OpaqueOffset {
                path,
                sha256: sha256_hex(raw),
                bytes: raw.len() as u64,
            }
        };
        let mut position = OraclePosition {
            protocol: "oo.c1.oracle-logminer-position/v1".into(),
            engine: "oracle".into(),
            commit_scn: "123456".into(),
            transaction_id: "owned_unit".into(),
            event_count: 101000,
            end_event_count: 101000,
            event_order_first: 1,
            event_order_last: 101000,
            events_sha256: "a".repeat(64),
            offset_before: make("before", b"opaque-before"),
            offset_after: make("after", b"opaque-after"),
        };
        assert!(position.validate().is_ok());
        position.event_order_last -= 1;
        assert!(position.validate().is_err());
        position.event_order_last += 1;
        position.end_event_count -= 1;
        assert!(position.validate().is_err());
        position.end_event_count += 1;
        std::fs::write(&position.offset_after.path, b"changed-last").unwrap();
        assert!(position.validate().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn batch_artifact_stream_checks_beyond_32_mib_and_the_selected_frame() {
        let root = test_root("stream");
        let path = root.join("batches.jsonl");
        let mut batch = crate::c1_typed::Batch {
            protocol: crate::c1_typed::PROTOCOL.into(),
            sequence: 1,
            previous_position: "0".into(),
            position: "0".into(),
            kind: "fill".into(),
            parts: vec![crate::c1_typed::Part {
                table: "UNIT.T".into(),
                path: "/unit/part.arrow".into(),
                sha256: "a".repeat(64),
                bytes: 1,
                rows: 0,
                spec: crate::c1_typed::TableSpec {
                    columns: vec![crate::c1_typed::Column {
                        name: "id".into(),
                        oid: 20,
                        typmod: -1,
                        nullable: false,
                    }],
                    keys: vec![0],
                },
            }],
        };
        let original = batch.clone();
        let mut hash = Sha256::new();
        let mut last_offset = 0;
        let mut file = File::create(&path).unwrap();
        for sequence in 1..=101000 {
            batch.sequence = sequence;
            let mut raw =
                serde_json::to_vec(&serde_json::json!({"kind":"typed_batch","batch":batch}))
                    .unwrap();
            raw.push(b'\n');
            last_offset = file.metadata().unwrap().len();
            file.write_all(&raw).unwrap();
            hash.update(&raw);
        }
        drop(file);
        assert!(std::fs::metadata(&path).unwrap().len() > 32 * 1024 * 1024);
        let digest = format!("{:x}", hash.finalize());
        assert!(verify_batch_stream(&path, &digest, &original).is_ok());
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(last_offset)).unwrap();
        file.write_all(b"!").unwrap();
        drop(file);
        assert!(
            verify_batch_stream(&path, &digest, &original).is_err(),
            "late corruption cannot hide after the selected frame"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_capture_document_still_requires_valid_physical_identity() {
        let make = |id: &str| serde_json::json!({"boundary":{"identity":id}});
        let id = format!("42|100|{}|PDB|DB|APP", "A".repeat(32));
        assert_eq!(
            CapturePosition::identity_from_document(&make(&id))
                .unwrap()
                .measured_id()
                .unwrap(),
            format!("oracle.42.100.{}", "a".repeat(32))
        );
        for wrong in [
            "42|100",
            "42|100|bad-guid|PDB|DB|APP",
            "0|100|AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA|PDB|DB|APP",
        ] {
            assert!(CapturePosition::identity_from_document(&make(wrong)).is_err());
        }
    }

    #[test]
    fn engine_identity_cannot_masquerade_as_mysql_and_reset_changes_lineage() {
        let a = Identity::Oracle {
            dbid: "42".into(),
            resetlogs_change: "100".into(),
            pdb_guid: "A".repeat(32),
            pdb_name: "PDB".into(),
            db_unique_name: "DB".into(),
        };
        assert_eq!(a.engine(), "oracle");
        assert!(a.measured_id().unwrap().starts_with("oracle."));
        let mut b = a.clone();
        if let Identity::Oracle {
            resetlogs_change, ..
        } = &mut b
        {
            *resetlogs_change = "200".into();
        }
        assert_ne!(a.measured_id().unwrap(), b.measured_id().unwrap());
        assert!(
            serde_json::from_str::<Identity>(r#"{"engine":"oracle","server_uuid":"mysql"}"#)
                .is_err()
        );
        assert!(!decimal("01"));
        assert!(decimal("2126801"));
    }

    #[test]
    fn opaque_checkpoint_bytes_and_complete_end_are_required_not_scn_plus_one() {
        let root = std::env::temp_dir().join(format!("oo-c1-offset-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let make = |name: &str, bytes: &[u8]| {
            let path = root.join(name);
            std::fs::write(&path, bytes).unwrap();
            OpaqueOffset {
                path,
                sha256: sha256_hex(bytes),
                bytes: bytes.len() as u64,
            }
        };
        let mut position = OraclePosition {
            protocol: "oo.c1.oracle-logminer-position/v1".into(),
            engine: "oracle".into(),
            commit_scn: "2133689".into(),
            transaction_id: "01001800ed010000".into(),
            event_count: 2,
            end_event_count: 2,
            event_order_first: 1,
            event_order_last: 2,
            events_sha256: "a".repeat(64),
            offset_before: make("before", b"unit opaque before"),
            offset_after: make("after", b"unit opaque after"),
        };
        let bound = position.bound().unwrap();
        assert!(bound["offset_before"].get("path").is_none());
        position.commit_scn = "90000000000000000000000000000000000000".into();
        assert!(
            position.validate().is_ok(),
            "not an apply ordinal or u64 GTID"
        );
        position.end_event_count = 1;
        assert!(position.validate().is_err());
        position.end_event_count = 2;
        std::fs::write(&position.offset_after.path, b"tampered").unwrap();
        assert!(position.validate().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
