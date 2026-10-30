use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

pub const PROTOCOL: &str = "oo.mysql.c5-multipart/v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub index: usize,
    pub total: usize,
    pub chunk_id: usize,
    pub rows: u64,
    pub arrow_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub protocol: String,
    pub source_id: String,
    pub source_generation: i64,
    pub occurrence: String,
    pub schema: String,
    pub table: String,
    pub schema_sha256: String,
    pub capture_config_sha256: String,
    pub manifest_sha256: String,
    pub chunks: Value,
    pub part_count: usize,
    pub parts: Vec<Part>,
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Manifest {
    pub fn parse(value: &Value) -> Result<Self, &'static str> {
        let this: Self = serde_json::from_value(value.clone()).map_err(|_| "multipart_scope")?;
        if this.protocol != PROTOCOL
            || this.source_generation < 1
            || this.occurrence.is_empty()
            || [&this.source_id, &this.schema, &this.table, &this.occurrence]
                .iter()
                .any(|s| s.is_empty() || s.contains('\0'))
            || [
                &this.schema_sha256,
                &this.capture_config_sha256,
                &this.manifest_sha256,
            ]
            .iter()
            .any(|s| {
                s.len() != 64
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            || this.part_count == 0
            || this.part_count != this.parts.len()
            || digest(&serde_json::to_vec(&this.chunks).map_err(|_| "multipart_manifest")?)
                != this.manifest_sha256
        {
            return Err("multipart_scope");
        }
        let chunks = this.chunks.as_array().ok_or("multipart_manifest")?;
        if chunks.len() != this.part_count {
            return Err("multipart_count");
        }
        let mut previous = Value::Null;
        for (index, (part, chunk)) in this.parts.iter().zip(chunks).enumerate() {
            if part.index != index
                || part.chunk_id != index
                || part.total != this.part_count
                || part.rows == 0
                || part.rows > 100_000
                || chunk["table"] != this.table
                || chunk["chunk_id"].as_u64() != Some(index as u64)
                || chunk["rows"].as_u64() != Some(part.rows)
                || chunk["arrow_sha256"] != part.arrow_sha256
                || chunk["lo"] != previous
                || chunk["hi"].is_null()
                || part.arrow_sha256.len() != 64
                || !part
                    .arrow_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("multipart_order");
            }
            previous = chunk["hi"].clone();
        }
        Ok(this)
    }

    pub fn verify_part(&self, index: usize, bytes: &[u8]) -> Result<(), &'static str> {
        if bytes.len() > 32 * 1024 * 1024
            || digest(bytes) != self.parts.get(index).ok_or("multipart_index")?.arrow_sha256
        {
            return Err("multipart_digest");
        }
        Ok(())
    }

    pub fn read_part(&self, directory: &Path, index: usize) -> Result<Vec<u8>, &'static str> {
        let mut bytes = Vec::new();
        fs::File::open(directory.join(format!("{index}.arrow")))
            .map_err(|_| "multipart_missing")?
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "multipart_read")?;
        self.verify_part(index, &bytes)?;
        Ok(bytes)
    }

    pub fn verify_files(&self, directory: &Path) -> Result<(), &'static str> {
        let mut bytes = Vec::new();
        fs::File::open(directory.join("chunks.json"))
            .map_err(|_| "multipart_manifest")?
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "multipart_manifest")?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("multipart_manifest");
        }
        let chunks: Value = serde_json::from_slice(&bytes).map_err(|_| "multipart_manifest")?;
        if chunks != self.chunks {
            return Err("multipart_manifest");
        }
        for entry in fs::read_dir(directory).map_err(|_| "multipart_read")? {
            let name = entry.map_err(|_| "multipart_read")?.file_name();
            if let Some(stem) = name.to_str().and_then(|s| s.strip_suffix(".arrow")) {
                if stem == "table-0" {
                    continue;
                }
                let number = stem.parse::<usize>().map_err(|_| "multipart_extra")?;
                if number >= self.part_count || stem != number.to_string() {
                    return Err("multipart_extra");
                }
            }
        }
        for index in 0..self.part_count {
            self.read_part(directory, index)?;
        }
        Ok(())
    }
}
