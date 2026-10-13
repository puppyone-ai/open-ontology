use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use oo_pipeline::target_installer::sha256_hex;

use crate::error::MirrorError;

pub struct Header {
    pub plan_sha256: String,
    pub generation: u64,
    pub snapshot_id: String,
    pub config_digest: String,
    pub type_digest: String,
    pub input_digest: String,
    pub schema_digest: String,
}

#[derive(Clone)]
pub struct BatchBlob {
    pub table_index: u32,
    pub rows: u64,
    pub batch_id: String,
    pub payload_digest: String,
    pub applied_pos: String,
    pub row_hashes: Vec<u8>,
    pub copy_bytes: Vec<u8>,
}

pub struct Loaded {
    pub header: Header,
    pub batches: Vec<BatchBlob>,
}

pub enum SpoolState {
    Absent,
    Partial,
    Complete,
}

pub fn directory(manifest: &Path) -> Result<PathBuf, MirrorError> {
    let name = manifest
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| MirrorError::new("config", "manifest_path"))?;
    let parent = manifest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| MirrorError::new("config", "manifest_path"))?;
    Ok(parent.join(format!("{name}.spool")))
}

pub fn inspect(dir: &Path) -> Result<SpoolState, MirrorError> {
    if !owned_spool(dir) {
        return Err(MirrorError::new("config", "spool"));
    }
    match fs::symlink_metadata(dir) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(SpoolState::Absent),
        Err(_) => Err(MirrorError::new("config", "spool")),
        Ok(meta) if meta.file_type().is_symlink() => Err(MirrorError::new("manifest_conflict", "symlink")),
        Ok(meta) if meta.is_dir() => {
            let marker = dir.join("COMPLETE");
            match fs::symlink_metadata(&marker) {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(SpoolState::Partial),
                Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(SpoolState::Complete),
                _ => Err(MirrorError::new("manifest_conflict", "spool")),
            }
        }
        Ok(_) => Err(MirrorError::new("manifest_conflict", "spool")),
    }
}

pub fn discard(dir: &Path) -> Result<(), MirrorError> {
    if !owned_spool(dir) {
        return Err(MirrorError::new("config", "spool"));
    }
    match fs::symlink_metadata(dir) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(meta) if meta.file_type().is_symlink() => Err(MirrorError::new("manifest_conflict", "symlink")),
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(dir).map_err(|_| MirrorError::new("config", "spool")),
        _ => Err(MirrorError::new("manifest_conflict", "spool")),
    }
}

pub struct Writer {
    dir: PathBuf,
    header: Header,
    count: u32,
}

pub fn begin(dir: &Path, header: Header) -> Result<Writer, MirrorError> {
    if !owned_spool(dir) {
        return Err(MirrorError::new("config", "spool"));
    }
    if fs::symlink_metadata(dir).is_ok() {
        return Err(MirrorError::new("manifest_conflict", "spool"));
    }
    fs::create_dir(dir).map_err(|_| MirrorError::new("config", "spool"))?;
    let body = header_text(&header);
    write_new(&dir.join("header"), body.as_bytes())?;
    Ok(Writer {
        dir: dir.to_path_buf(),
        header,
        count: 0,
    })
}

impl Writer {
    pub fn push(&mut self, batch: &BatchBlob) -> Result<(), MirrorError> {
        let name = format!("batch-{:08}", self.count);
        let path = self.dir.join(name);
        let mut bytes = Vec::new();
        write_u32(&mut bytes, batch.table_index);
        write_u64(&mut bytes, batch.rows);
        write_str(&mut bytes, &batch.batch_id)?;
        write_str(&mut bytes, &batch.payload_digest)?;
        write_str(&mut bytes, &batch.applied_pos)?;
        write_bytes(&mut bytes, &batch.row_hashes)?;
        write_bytes(&mut bytes, &batch.copy_bytes)?;
        write_new(&path, &bytes)?;
        self.count = self.count.saturating_add(1);
        Ok(())
    }

    pub fn complete(self) -> Result<(), MirrorError> {
        write_new(&self.dir.join("COMPLETE"), b"complete\n")?;
        let _ = self.header;
        Ok(())
    }
}

pub fn load_complete(dir: &Path, plan_sha256: &str, generation: u64) -> Result<Option<Loaded>, MirrorError> {
    match inspect(dir)? {
        SpoolState::Absent | SpoolState::Partial => Ok(None),
        SpoolState::Complete => {
            let header = read_header(&dir.join("header"))?;
            if header.plan_sha256 != plan_sha256 || header.generation != generation {
                return Err(MirrorError::new("manifest_conflict", "spool"));
            }
            let mut batches = Vec::new();
            let mut index = 0u32;
            loop {
                let path = dir.join(format!("batch-{index:08}"));
                match fs::symlink_metadata(&path) {
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
                    Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
                        batches.push(read_batch(&path)?);
                    }
                    _ => return Err(MirrorError::new("manifest_conflict", "spool")),
                }
                index = index.saturating_add(1);
            }
            for batch in &batches {
                let quality = quality_bytes(&batch.batch_id, batch.rows, &header.input_digest);
                let mut mixed = batch.copy_bytes.clone();
                mixed.extend_from_slice(&quality);
                if sha256_hex(&mixed) != batch.payload_digest {
                    return Err(MirrorError::new("manifest_tamper", "spool"));
                }
            }
            Ok(Some(Loaded { header, batches }))
        }
    }
}

pub fn quality_bytes(batch_id: &str, accepted: u64, input_digest: &str) -> Vec<u8> {
    format!("full-import/v1 quality\nbatch\t{batch_id}\t{accepted}\t0\t{input_digest}\n").into_bytes()
}

fn owned_spool(dir: &Path) -> bool {
    dir.is_absolute()
        && dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".spool") && !name.contains('/') && name != ".spool")
}

fn header_text(header: &Header) -> String {
    format!(
        "plan={}\ngeneration={}\nsnapshot={}\nconfig={}\ntype={}\ninput={}\nschema={}\n",
        header.plan_sha256,
        header.generation,
        header.snapshot_id,
        header.config_digest,
        header.type_digest,
        header.input_digest,
        header.schema_digest
    )
}

fn read_header(path: &Path) -> Result<Header, MirrorError> {
    let text = fs::read_to_string(path).map_err(|_| MirrorError::new("config", "spool"))?;
    let mut plan = None;
    let mut generation = None;
    let mut snapshot = None;
    let mut config = None;
    let mut kind = None;
    let mut input = None;
    let mut schema = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            return Err(MirrorError::new("manifest_tamper", "spool"));
        };
        match key {
            "plan" => plan = Some(value.to_string()),
            "generation" => {
                generation = Some(value.parse::<u64>().map_err(|_| MirrorError::new("manifest_tamper", "spool"))?);
            }
            "snapshot" => snapshot = Some(value.to_string()),
            "config" => config = Some(value.to_string()),
            "type" => kind = Some(value.to_string()),
            "input" => input = Some(value.to_string()),
            "schema" => schema = Some(value.to_string()),
            _ => return Err(MirrorError::new("manifest_tamper", "spool")),
        }
    }
    Ok(Header {
        plan_sha256: plan.ok_or_else(|| MirrorError::new("manifest_tamper", "spool"))?,
        generation: generation.ok_or_else(|| MirrorError::new("manifest_tamper", "spool"))?,
        snapshot_id: snapshot.ok_or_else(|| MirrorError::new("manifest_tamper", "spool"))?,
        config_digest: config.ok_or_else(|| MirrorError::new("manifest_tamper", "spool"))?,
        type_digest: kind.ok_or_else(|| MirrorError::new("manifest_tamper", "spool"))?,
        input_digest: input.ok_or_else(|| MirrorError::new("manifest_tamper", "spool"))?,
        schema_digest: schema.ok_or_else(|| MirrorError::new("manifest_tamper", "spool"))?,
    })
}

fn read_batch(path: &Path) -> Result<BatchBlob, MirrorError> {
    let bytes = fs::read(path).map_err(|_| MirrorError::new("config", "spool"))?;
    let mut cursor = Cursor { bytes: &bytes, index: 0 };
    Ok(BatchBlob {
        table_index: cursor.u32()?,
        rows: cursor.u64()?,
        batch_id: cursor.string()?,
        payload_digest: cursor.string()?,
        applied_pos: cursor.string()?,
        row_hashes: cursor.bytes()?,
        copy_bytes: cursor.bytes()?,
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl Cursor<'_> {
    fn u32(&mut self) -> Result<u32, MirrorError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, MirrorError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn string(&mut self) -> Result<String, MirrorError> {
        let bytes = self.bytes()?;
        String::from_utf8(bytes).map_err(|_| MirrorError::new("manifest_tamper", "spool"))
    }

    fn bytes(&mut self) -> Result<Vec<u8>, MirrorError> {
        let len = usize::try_from(self.u32()?).map_err(|_| MirrorError::new("manifest_tamper", "spool"))?;
        Ok(self.take(len)?.to_vec())
    }

    fn take(&mut self, len: usize) -> Result<&[u8], MirrorError> {
        let end = self.index.checked_add(len).ok_or_else(|| MirrorError::new("manifest_tamper", "spool"))?;
        if end > self.bytes.len() {
            return Err(MirrorError::new("manifest_tamper", "spool"));
        }
        let slice = &self.bytes[self.index..end];
        self.index = end;
        Ok(slice)
    }
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_str(out: &mut Vec<u8>, value: &str) -> Result<(), MirrorError> {
    write_bytes(out, value.as_bytes())
}

fn write_bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<(), MirrorError> {
    let len = u32::try_from(value.len()).map_err(|_| MirrorError::new("budget", "max_bytes"))?;
    write_u32(out, len);
    out.extend_from_slice(value);
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), MirrorError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| MirrorError::new("config", "spool"))?;
    file.write_all(bytes).map_err(|_| MirrorError::new("config", "spool"))?;
    file.sync_all().map_err(|_| MirrorError::new("config", "spool"))?;
    Ok(())
}
