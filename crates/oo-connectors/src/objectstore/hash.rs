//! Streaming dual hash (BLAKE3 content key + SHA-256 for external checks).

use std::io::Read;

/// Bytes per read when hashing a stream.
pub const HASH_CHUNK: usize = 64 * 1024;

/// Final digests, lowercase hex.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DualDigest {
    pub b3: String,
    pub sha256: String,
    pub len: u64,
}

/// Incremental BLAKE3 + SHA-256 over the same bytes.
pub struct DualHasher {
    b3: blake3::Hasher,
    sha: sha2::Sha256,
    len: u64,
}

impl Default for DualHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl DualHasher {
    pub fn new() -> Self {
        use sha2::Digest;
        Self {
            b3: blake3::Hasher::new(),
            sha: sha2::Sha256::new(),
            len: 0,
        }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        use sha2::Digest;
        self.b3.update(bytes);
        self.sha.update(bytes);
        self.len = self.len.saturating_add(bytes.len() as u64);
    }

    pub fn finalize(self) -> DualDigest {
        use sha2::Digest;
        let sha = self.sha.finalize();
        let mut sha_hex = String::with_capacity(64);
        for byte in sha {
            sha_hex.push_str(&format!("{byte:02x}"));
        }
        DualDigest {
            b3: self.b3.finalize().to_hex().to_string(),
            sha256: sha_hex,
            len: self.len,
        }
    }
}

/// Hash a reader to the end in bounded chunks.
pub fn hash_reader<R: Read>(mut reader: R) -> std::io::Result<DualDigest> {
    let mut hasher = DualHasher::new();
    let mut buf = vec![0u8; HASH_CHUNK];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(hasher.finalize())
}
