//! SHA-256 manifest for the a198 query-pg evidence.
//! The digests are computed here. No external hash command is the oracle.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256(message: &[u8]) -> [u8; 32] {
    let mut state = H0;
    let bit_len = (message.len() as u64).wrapping_mul(8);
    let mut offset = 0;
    while message.len() - offset >= 64 {
        let mut block = [0u8; 64];
        block.copy_from_slice(&message[offset..offset + 64]);
        compress(&mut state, &block);
        offset += 64;
    }
    let mut tail = message[offset..].to_vec();
    tail.push(0x80);
    while (tail.len() % 64) != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in tail.chunks(64) {
        let mut block = [0u8; 64];
        block.copy_from_slice(chunk);
        compress(&mut state, &block);
    }
    let mut out = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut words = [0u32; 64];
    for index in 0..16 {
        let start = index * 4;
        words[index] = u32::from_be_bytes(block[start..start + 4].try_into().expect("word"));
    }
    for index in 16..64 {
        let s0 = words[index - 15].rotate_right(7)
            ^ words[index - 15].rotate_right(18)
            ^ (words[index - 15] >> 3);
        let s1 = words[index - 2].rotate_right(17)
            ^ words[index - 2].rotate_right(19)
            ^ (words[index - 2] >> 10);
        words[index] = words[index - 16]
            .wrapping_add(s0)
            .wrapping_add(words[index - 7])
            .wrapping_add(s1);
    }
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];
    for index in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(choose)
            .wrapping_add(K[index])
            .wrapping_add(words[index]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(majority);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn expect_vector(label: &str, message: &[u8], expect: &str) -> Result<(), String> {
    let got = hex(&sha256(message));
    if got != expect {
        return Err(format!("{label} got {got}"));
    }
    Ok(())
}

fn self_test() -> Result<(), String> {
    expect_vector(
        "empty",
        b"",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    )?;
    expect_vector(
        "abc",
        b"abc",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    )?;
    expect_vector(
        "448",
        b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    )?;
    Ok(())
}

fn hash_file(path: &Path) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(sha256(&bytes))
}

fn collect(path: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    if path.is_symlink() {
        return Ok(());
    }
    if path.is_dir() {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            entries.push(entry?.path());
        }
        entries.sort();
        for entry in entries {
            collect(&entry, root, out)?;
        }
        return Ok(());
    }
    if !path.is_file() {
        return Ok(());
    }
    let name = path.file_name().and_then(|item| item.to_str()).unwrap_or("");
    if name == "SHA256SUMS" || name == "sha256-manifest" {
        return Ok(());
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    out.push((relative.to_string_lossy().replace('\\', "/"), path.to_path_buf()));
    Ok(())
}

fn main() -> ExitCode {
    if let Err(err) = self_test() {
        eprintln!("vector {err}");
        return ExitCode::from(1);
    }
    let mut args = env::args().skip(1);
    let sums = PathBuf::from(args.next().expect("sums path"));
    let vectors = PathBuf::from(args.next().expect("vectors path"));
    let root = PathBuf::from(args.next().expect("display root"));
    let vector_text = "\
empty e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
abc ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
448 248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1
";
    if let Err(err) = fs::write(&vectors, vector_text) {
        eprintln!("write vectors: {err}");
        return ExitCode::from(1);
    }
    let mut files = Vec::new();
    for arg in args {
        if let Err(err) = collect(Path::new(&arg), &root, &mut files) {
            eprintln!("collect {arg}: {err}");
            return ExitCode::from(1);
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files.dedup_by(|left, right| left.0 == right.0);
    let mut body = String::new();
    for (relative, path) in &files {
        let digest = match hash_file(path) {
            Ok(digest) => digest,
            Err(err) => {
                eprintln!("hash {relative}: {err}");
                return ExitCode::from(1);
            }
        };
        body.push_str(&hex(&digest));
        body.push_str("  ");
        body.push_str(relative);
        body.push('\n');
    }
    if let Err(err) = fs::write(&sums, &body) {
        eprintln!("write sums: {err}");
        return ExitCode::from(1);
    }
    println!("sha256_vectors=ok files={}", files.len());
    ExitCode::SUCCESS
}
