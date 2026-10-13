use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0_u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn random_bytes(len: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0_u8; len];
    let mut file = std::fs::File::open("/dev/urandom")?;
    use std::io::Read;
    file.read_exact(&mut buf)?;
    Ok(buf)
}
