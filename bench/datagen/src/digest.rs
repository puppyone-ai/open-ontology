use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{Result, err};

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes))
}

pub fn hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub struct HashWriter<W> {
    inner: W,
    hash: Sha256,
    len: u64,
}

impl<W: Write> HashWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hash: Sha256::new(),
            len: 0,
        }
    }

    pub fn finish(mut self) -> Result<(W, u64, String)> {
        self.inner.flush()?;
        let digest = self.hash.finalize();
        Ok((self.inner, self.len, hex(digest)))
    }
}

impl<W: Write> Write for HashWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        self.len += u64::try_from(n).unwrap_or(u64::MAX);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut len = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        len += u64::try_from(n).map_err(|_| err("file longer than u64"))?;
    }
    Ok((len, hex(hash.finalize())))
}

/// Tree digest over content files. Manifest bytes are excluded because they
/// store this digest. The sidecar hash is excluded for the same reason.
pub fn tree_preimage(entries: &[(String, u64, String)]) -> String {
    let mut hash = Sha256::new();
    for (path, len, digest) in entries {
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update(digest.as_bytes());
        hash.update([0]);
        hash.update(len.to_string().as_bytes());
        hash.update(b"\n");
    }
    hex(hash.finalize())
}
