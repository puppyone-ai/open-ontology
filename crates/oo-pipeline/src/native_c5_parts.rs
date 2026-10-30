//! Streaming transport for growing C5 ranges. Row/part totals are not quotas.
//! Index acceptance is separate from admission and atomic target publication.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata},
    io::{BufRead, BufReader, Read},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

pub const PROTOCOL: &str = "oo.mysql.c5-range-parts/v1";
pub const PART_SCOPE_PROTOCOL: &str = "oo.pipeline.native-c5-range-part/v1";
pub const RANGE_PROTOCOL: &str = "oo.pipeline.native-c5-range-parts/v1";
const MAX_INDEX_LINE: u64 = 65_536;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub ordinal: u64,
    pub arrow: FileRef,
    pub row_count: u64,
}

/// Matches _oo.c5_key_identity, including type tags and component lengths.
pub fn key_identity(key: &crate::native_c5_range::TypedKey) -> Result<Vec<u8>> {
    use crate::native_c5_range::KeyComponent;
    if key.is_empty() {
        return Err("c5_parts_key");
    }
    let mut result = Vec::new();
    for component in key {
        let (tag, payload) = match component {
            KeyComponent::Int(value) => (0, value.to_be_bytes().to_vec()),
            KeyComponent::Text(value) | KeyComponent::Bytes(value) => {
                if !value.len().is_multiple_of(2)
                    || !value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                {
                    return Err("c5_parts_key");
                }
                let bytes = value.as_bytes().chunks_exact(2)
                    .map(|pair| {
                        let digit = |c| if c <= b'9' { c - b'0' } else { c - b'a' + 10 };
                        digit(pair[0]) * 16 + digit(pair[1])
                    }).collect();
                (if matches!(component, KeyComponent::Text(_)) { 1 } else { 2 }, bytes)
            }
        };
        let size = u32::try_from(payload.len()).ok().and_then(|v| v.checked_add(1))
            .ok_or("c5_parts_key")?;
        result.extend_from_slice(&size.to_be_bytes());
        result.push(tag);
        result.extend_from_slice(&payload);
    }
    Ok(result)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RangeIdentity {
    pub source_engine: String,
    pub source_id: String,
    pub schema: String,
    pub table: String,
    pub schema_fingerprint: String,
    pub chunk_topology_revision: i64,
    pub chunk_id: String,
    pub lo: Option<crate::native_c5_range::TypedKey>,
    pub hi: Option<crate::native_c5_range::TypedKey>,
    pub prior_membership_revision: i64,
    pub parent_generation: i64,
    pub source_generation: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub kind: String,
    pub value: String,
}

/// This scope is bound by the real admission sealer to ONE captured Arrow part.
/// It is never sufficient by itself to publish or replace the complete range.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PartScope {
    pub protocol: String,
    pub range: RangeIdentity,
    pub parts_index: FileRef,
    pub part_count: u64,
    pub total_rows: u64,
    pub part: Part,
    pub snapshot: Snapshot,
}

impl PartScope {
    pub fn parse(value: &serde_json::Value) -> Result<Self> {
        let scope: Self = serde_json::from_value(value.clone()).map_err(|_| "c5_part_scope")?;
        if serde_json::to_value(&scope).map_err(|_| "c5_part_scope")? != *value {
            return Err("c5_part_scope");
        }
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<()> {
        let range = &self.range;
        if self.protocol != PART_SCOPE_PROTOCOL
            || range.source_engine != "mysql"
            || range.parent_generation < 0
            || range.parent_generation.checked_add(1) != Some(range.source_generation)
            || self.part_count == 0
            || self.part.ordinal >= self.part_count
            || self.part.row_count > self.total_rows
            || (self.total_rows == 0 && (self.part_count != 1 || self.part.row_count != 0))
            || (self.total_rows != 0 && self.part.row_count == 0)
            || [&self.snapshot.kind, &self.snapshot.value]
                .iter()
                .any(|s| s.is_empty() || s.contains('\0'))
        {
            return Err("c5_part_scope");
        }
        for reference in [&self.parts_index, &self.part.arrow] {
            let path = Path::new(&reference.path);
            if !path.is_absolute()
                || reference.path.contains('\0')
                || path.components().any(|c| c == Component::ParentDir)
                || !valid_digest(&reference.sha256)
                || reference.bytes == 0
            {
                return Err("c5_part_scope");
            }
        }
        if Path::new(&self.part.arrow.path)
            != Path::new(&self.parts_index.path)
                .parent()
                .ok_or("c5_part_scope")?
                .join(format!("part-{}.arrow", self.part.ordinal))
        {
            return Err("c5_part_scope");
        }
        // Reuse the closed key/topology validator, without inventing a receipt.
        let identity = crate::native_c5_range::RangeScope {
            protocol: crate::native_c5_range::REQUEST_PROTOCOL.into(),
            source_engine: range.source_engine.clone(),
            capture_method: "c5_diff".into(),
            source_id: range.source_id.clone(),
            schema: range.schema.clone(),
            table: range.table.clone(),
            schema_fingerprint: range.schema_fingerprint.clone(),
            chunk_topology_revision: range.chunk_topology_revision,
            chunk_id: range.chunk_id.clone(),
            lo: range.lo.clone(),
            hi: range.hi.clone(),
            prior_membership_revision: range.prior_membership_revision,
            parent_generation: range.parent_generation,
            source_generation: range.source_generation,
            arrow_sha256: self.parts_index.sha256.clone(),
            row_count: 0,
            delete_detection: "reconcile".into(),
            position_codec: "snapshot-zero/v1".into(),
            applied_pos: "0".into(),
            members: Vec::new(),
            oracle_snapshot: None,
        };
        crate::native_c5_range::RangeScope::parse(
            &serde_json::to_value(identity).map_err(|_| "c5_part_scope")?,
        )
        .map_err(|_| "c5_part_scope")?;
        Ok(())
    }

    pub fn same_capture(&self, other: &Self) -> bool {
        self.protocol == other.protocol
            && self.range == other.range
            && self.parts_index == other.parts_index
            && self.part_count == other.part_count
            && self.total_rows == other.total_rows
            && self.snapshot == other.snapshot
    }
}

pub fn range_scope(part: &PartScope, task: &str) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(&part.range).map_err(|_| "c5_parts_scope")?;
    for (key, item) in [
        ("protocol", serde_json::json!(RANGE_PROTOCOL)),
        ("capture_method", serde_json::json!("c5_diff")),
        ("arrow_sha256", serde_json::json!(part.parts_index.sha256)),
        ("row_count", serde_json::json!(part.total_rows)),
        ("delete_detection", serde_json::json!("reconcile")),
        ("position_codec", serde_json::json!("snapshot-zero/v1")),
        ("applied_pos", serde_json::json!("0")),
        ("parts_task", serde_json::json!(task)),
    ] {
        value[key] = item;
    }
    validate_range_scope(&value)?;
    Ok(value)
}

/// Validate the compact complete-range identity without constructing members.
pub fn validate_range_scope(value: &serde_json::Value) -> Result<RangeIdentity> {
    if value["protocol"] != RANGE_PROTOCOL
        || value.as_object().is_none_or(|v| v.len() != 20)
        || value["parts_task"].as_str().is_none_or(|v| {
            v.len() != 36 || !v.bytes().enumerate().all(|(i, c)|
                if [8, 13, 18, 23].contains(&i) { c == b'-' } else { c.is_ascii_hexdigit() })
        })
        || value["row_count"].as_i64().is_none_or(|v| v < 0)
    {
        return Err("c5_parts_scope");
    }
    let mut legacy = value.clone();
    legacy.as_object_mut().ok_or("c5_parts_scope")?.remove("parts_task");
    legacy["protocol"] = serde_json::json!(crate::native_c5_range::REQUEST_PROTOCOL);
    legacy["row_count"] = serde_json::json!(0);
    legacy["members"] = serde_json::json!([]);
    crate::native_c5_range::RangeScope::parse(&legacy).map_err(|_| "c5_parts_scope")?;
    for key in ["protocol", "capture_method", "arrow_sha256", "row_count",
        "delete_detection", "position_codec", "applied_pos", "members"] {
        legacy.as_object_mut().ok_or("c5_parts_scope")?.remove(key);
    }
    serde_json::from_value(legacy).map_err(|_| "c5_parts_scope")
}

/// Bounded metadata pages; the number of lines is deliberately unrestricted.
pub struct JsonLines {
    reader: BufReader<File>,
    reference: FileRef,
    before: Metadata,
    hash: Sha256,
    bytes: u64,
}

impl JsonLines {
    pub fn open(reference: FileRef) -> Result<Self> {
        let (file, before) = open_private(&reference)?;
        Ok(Self { reader: BufReader::new(file), reference, before, hash: Sha256::new(), bytes: 0 })
    }

    pub fn next_value(&mut self) -> Result<Option<serde_json::Value>> {
        let mut line = Vec::new();
        self.reader.by_ref().take(1_048_577).read_until(b'\n', &mut line)
            .map_err(|_| "c5_parts_read")?;
        if line.is_empty() { return Ok(None); }
        if line.len() > 1_048_576 || line.last() != Some(&b'\n') {
            return Err("c5_parts_line");
        }
        self.bytes = self.bytes.checked_add(line.len() as u64).ok_or("c5_parts_size")?;
        if self.bytes > self.reference.bytes { return Err("c5_parts_size"); }
        self.hash.update(&line);
        serde_json::from_slice(&line).map(Some).map_err(|_| "c5_parts_line")
    }

    pub fn finish(mut self) -> Result<()> {
        if self.next_value()?.is_some() || self.bytes != self.reference.bytes
            || !same_file(&self.before, &self.reader.get_ref().metadata().map_err(|_| "c5_parts_file")?)
            || format!("{:x}", self.hash.finalize()) != self.reference.sha256 {
            return Err("c5_parts_digest");
        }
        Ok(())
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn same_file(a: &Metadata, b: &Metadata) -> bool {
    (
        a.dev(),
        a.ino(),
        a.len(),
        a.mode(),
        a.mtime(),
        a.mtime_nsec(),
    ) == (
        b.dev(),
        b.ino(),
        b.len(),
        b.mode(),
        b.mtime(),
        b.mtime_nsec(),
    )
}

fn open_private(reference: &FileRef) -> Result<(File, Metadata)> {
    let path = Path::new(&reference.path);
    if !path.is_absolute()
        || reference.path.contains('\0')
        || path.components().any(|c| c == Component::ParentDir)
        || !valid_digest(&reference.sha256)
        || reference.bytes == 0
    {
        return Err("c5_parts_reference");
    }
    let before = fs::symlink_metadata(path).map_err(|_| "c5_parts_file")?;
    if !before.is_file() || before.mode() & 0o077 != 0 || before.len() != reference.bytes {
        return Err("c5_parts_file");
    }
    let file = File::open(path).map_err(|_| "c5_parts_file")?;
    if !same_file(&before, &file.metadata().map_err(|_| "c5_parts_file")?) {
        return Err("c5_parts_file_changed");
    }
    Ok((file, before))
}

/// Verify bytes without retaining an Arrow part in memory.
pub fn verify_part(part: &Part) -> Result<()> {
    verify_file(&part.arrow)
}

pub fn verify_file(reference: &FileRef) -> Result<()> {
    let (mut file, before) = open_private(reference)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65_536];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buffer).map_err(|_| "c5_parts_read")?;
        if n == 0 {
            break;
        }
        size = size.checked_add(n as u64).ok_or("c5_parts_size")?;
        if size > reference.bytes {
            return Err("c5_parts_file_changed");
        }
        hash.update(&buffer[..n]);
    }
    if size != reference.bytes
        || format!("{:x}", hash.finalize()) != reference.sha256
        || !same_file(&before, &file.metadata().map_err(|_| "c5_parts_file")?)
    {
        return Err("c5_parts_digest");
    }
    Ok(())
}

pub struct PartsIndex {
    reader: BufReader<File>,
    reference: FileRef,
    before: Metadata,
    directory: PathBuf,
    hash: Sha256,
    expected_parts: u64,
    expected_rows: u64,
    parts: u64,
    rows: u64,
    bytes: u64,
    eof: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CompleteIndex {
    pub parts: u64,
    pub rows: u64,
    pub sha256: String,
}

impl PartsIndex {
    pub fn open(reference: FileRef, expected_parts: u64, expected_rows: u64) -> Result<Self> {
        if expected_parts == 0 || (expected_rows == 0 && expected_parts != 1) {
            return Err("c5_parts_count");
        }
        let (file, before) = open_private(&reference)?;
        let directory = Path::new(&reference.path)
            .parent()
            .ok_or("c5_parts_reference")?
            .to_owned();
        Ok(Self {
            reader: BufReader::new(file),
            reference,
            before,
            directory,
            hash: Sha256::new(),
            expected_parts,
            expected_rows,
            parts: 0,
            rows: 0,
            bytes: 0,
            eof: false,
        })
    }

    pub fn next_part(&mut self) -> Result<Option<Part>> {
        if self.eof {
            return Ok(None);
        }
        // This bounds a metadata line, not data volume or number of parts.
        let mut line = Vec::new();
        self.reader
            .by_ref()
            .take(MAX_INDEX_LINE + 1)
            .read_until(b'\n', &mut line)
            .map_err(|_| "c5_parts_read")?;
        if line.is_empty() {
            self.eof = true;
            return Ok(None);
        }
        if line.len() as u64 > MAX_INDEX_LINE || line.last() != Some(&b'\n') {
            return Err("c5_parts_line");
        }
        self.bytes = self
            .bytes
            .checked_add(line.len() as u64)
            .ok_or("c5_parts_size")?;
        if self.bytes > self.reference.bytes || self.parts >= self.expected_parts {
            return Err("c5_parts_count");
        }
        self.hash.update(&line);
        let part: Part = serde_json::from_slice(&line).map_err(|_| "c5_parts_line")?;
        if part.ordinal != self.parts
            || Path::new(&part.arrow.path)
                != self.directory.join(format!("part-{}.arrow", self.parts))
            || !valid_digest(&part.arrow.sha256)
            || part.arrow.bytes == 0
            || (self.expected_rows != 0 && part.row_count == 0)
        {
            return Err("c5_parts_order");
        }
        self.rows = self
            .rows
            .checked_add(part.row_count)
            .ok_or("c5_parts_count")?;
        if self.rows > self.expected_rows {
            return Err("c5_parts_count");
        }
        self.parts += 1;
        Ok(Some(part))
    }

    /// Must succeed before any public generation or ACK can advance.
    pub fn finish(mut self) -> Result<CompleteIndex> {
        if self.next_part()?.is_some()
            || self.parts != self.expected_parts
            || self.rows != self.expected_rows
            || self.bytes != self.reference.bytes
            || !same_file(
                &self.before,
                &self
                    .reader
                    .get_ref()
                    .metadata()
                    .map_err(|_| "c5_parts_file")?,
            )
        {
            return Err("c5_parts_incomplete");
        }
        let sha256 = format!("{:x}", self.hash.finalize());
        if sha256 != self.reference.sha256 {
            return Err("c5_parts_digest");
        }
        Ok(CompleteIndex {
            parts: self.parts,
            rows: self.rows,
            sha256,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, os::unix::fs::OpenOptionsExt, time::SystemTime};

    #[test]
    fn compact_scope_keeps_total_rows_without_members_or_business_cap() {
        let scope = serde_json::json!({
            "protocol":RANGE_PROTOCOL,"source_engine":"mysql","capture_method":"c5_diff",
            "source_id":"sample","schema":"sample","table":"items","schema_fingerprint":"a".repeat(64),
            "chunk_topology_revision":1,"chunk_id":"0","lo":null,"hi":null,
            "prior_membership_revision":1,"parent_generation":1,"source_generation":2,
            "arrow_sha256":"b".repeat(64),"row_count":1_000_000_001_u64,
            "delete_detection":"reconcile","position_codec":"snapshot-zero/v1","applied_pos":"0",
            "parts_task":"33333333-3333-4333-8333-333333333333"
        });
        assert_eq!(validate_range_scope(&scope).unwrap().source_generation,2);
        for (field,value) in [
            ("members",serde_json::json!([])),("parts_task",serde_json::json!("invalid")),
            ("source_generation",serde_json::json!(4)),("row_count",serde_json::json!(-1)),
        ] {
            let mut invalid=scope.clone();invalid[field]=value;
            assert!(validate_range_scope(&invalid).is_err(),"{field}");
        }
    }

    #[test]
    fn admitted_index_must_be_complete_and_hash_bound() {
        let f=Fixture::new();
        let index=f.file("admitted.jsonl",b"{\"ordinal\":0}\n{\"ordinal\":1}\n");
        let mut reader=JsonLines::open(index.clone()).unwrap();
        assert_eq!(reader.next_value().unwrap().unwrap()["ordinal"],0);
        assert!(reader.finish().is_err());
        let mut reader=JsonLines::open(index.clone()).unwrap();
        while reader.next_value().unwrap().is_some() {}
        reader.finish().unwrap();
        let mut wrong=index;wrong.sha256="e".repeat(64);
        let mut reader=JsonLines::open(wrong).unwrap();
        while reader.next_value().unwrap().is_some() {}
        assert!(reader.finish().is_err());
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "oo-c5-parts-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed),
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
        fn file(&self, name: &str, bytes: &[u8]) -> FileRef {
            let path = self.0.join(name);
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&path)
                .unwrap();
            file.write_all(bytes).unwrap();
            FileRef {
                path: path.to_str().unwrap().into(),
                sha256: format!("{:x}", Sha256::digest(bytes)),
                bytes: bytes.len() as u64,
            }
        }
        fn index(&self, counts: &[u64]) -> FileRef {
            let mut bytes = Vec::new();
            for (ordinal, &row_count) in counts.iter().enumerate() {
                let part = Part {
                    ordinal: ordinal as u64,
                    arrow: self.file(
                        &format!("part-{ordinal}.arrow"),
                        b"transport-test-not-admitted-arrow",
                    ),
                    row_count,
                };
                serde_json::to_writer(&mut bytes, &part).unwrap();
                bytes.push(b'\n');
            }
            self.file("parts.jsonl", &bytes)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn staged_key_encoding_keeps_type_width_and_empty_bytes_distinct() {
        use crate::native_c5_range::KeyComponent::{Bytes, Int, Text};
        assert_eq!(key_identity(&vec![Int(1)]).unwrap(),
            vec![0,0,0,9,0,0,0,0,0,0,0,0,1]);
        assert_ne!(key_identity(&vec![Text("31".into())]).unwrap(),
            key_identity(&vec![Bytes("31".into())]).unwrap());
        assert_ne!(key_identity(&vec![Text("6162".into()),Text("63".into())]).unwrap(),
            key_identity(&vec![Text("61".into()),Text("6263".into())]).unwrap());
        assert_eq!(key_identity(&vec![Bytes(String::new())]).unwrap(), vec![0,0,0,1,2]);
        assert!(key_identity(&vec![Bytes("FG".into())]).is_err());
    }

    #[test]
    fn cumulative_rows_are_not_limited_to_a_source_page() {
        let f = Fixture::new();
        let pin = f.index(&[100_000, 100_000, 1]);
        let mut index = PartsIndex::open(pin.clone(), 3, 200_001).unwrap();
        let mut observed = 0;
        while let Some(part) = index.next_part().unwrap() {
            verify_part(&part).unwrap();
            observed += 1;
        }
        assert_eq!(observed, 3);
        assert_eq!(
            index.finish().unwrap(),
            CompleteIndex {
                parts: 3,
                rows: 200_001,
                sha256: pin.sha256
            }
        );
    }

    #[test]
    fn empty_replacement_still_requires_one_complete_part() {
        let f = Fixture::new();
        let pin = f.index(&[0]);
        assert!(PartsIndex::open(pin.clone(), 0, 0).is_err());
        let mut index = PartsIndex::open(pin, 1, 0).unwrap();
        verify_part(&index.next_part().unwrap().unwrap()).unwrap();
        assert_eq!(index.finish().unwrap().rows, 0);
    }

    #[test]
    fn early_completion_and_wrong_totals_are_refused() {
        let f = Fixture::new();
        let pin = f.index(&[2, 1]);
        let mut early = PartsIndex::open(pin.clone(), 2, 3).unwrap();
        early.next_part().unwrap();
        assert_eq!(early.finish().unwrap_err(), "c5_parts_incomplete");
        let mut wrong = PartsIndex::open(pin, 2, 2).unwrap();
        wrong.next_part().unwrap();
        assert_eq!(wrong.next_part().unwrap_err(), "c5_parts_count");
    }

    #[test]
    fn wrong_index_digest_and_corrupt_part_are_refused() {
        let f = Fixture::new();
        let mut pin = f.index(&[1]);
        pin.sha256 = "0".repeat(64);
        let mut index = PartsIndex::open(pin, 1, 1).unwrap();
        let part = index.next_part().unwrap().unwrap();
        fs::write(&part.arrow.path, b"corrupt").unwrap();
        assert!(verify_part(&part).is_err());
        assert_eq!(index.finish().unwrap_err(), "c5_parts_digest");
    }

    #[test]
    fn reordered_or_external_parts_cannot_be_spliced_in() {
        for external in [false, true] {
            let f = Fixture::new();
            let mut part = Part {
                ordinal: 1,
                arrow: f.file("part-0.arrow", b"x"),
                row_count: 1,
            };
            if external {
                part.ordinal = 0;
                part.arrow.path = "/private/other/part-0.arrow".into();
            }
            let mut bytes = serde_json::to_vec(&part).unwrap();
            bytes.push(b'\n');
            let mut index = PartsIndex::open(f.file("parts.jsonl", &bytes), 1, 1).unwrap();
            assert_eq!(index.next_part().unwrap_err(), "c5_parts_order");
        }
    }
}
