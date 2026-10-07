//! Std-only SHA-256 manifest.
//! Origin: a199 tools/sha256_manifest.rs
//! sha256 d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3
//! Change: optional third argument is a path prefix. Two arguments keep `./` paths.
//! The digest is FIPS 180-4 in this file. It does not shell out.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn main() -> ExitCode {
    if let Err(err) = run() {
        eprintln!("sha256_manifest: {err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let root = args.next().ok_or("usage: sha256_manifest <root> <output> [prefix]")?;
    let output = args.next().ok_or("usage: sha256_manifest <root> <output> [prefix]")?;
    let prefix = args.next().unwrap_or_default();
    if args.next().is_some() {
        return Err("usage: sha256_manifest <root> <output> [prefix]".to_owned());
    }
    check_sha256()?;
    let root = PathBuf::from(root);
    let output = PathBuf::from(output);
    let output_abs = absolute(&output)?;
    let mut rows = Vec::new();
    walk(&root, &root, &output_abs, &prefix, &mut rows)?;
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    let mut body = String::new();
    for (path, digest) in &rows {
        body.push_str(&hex(digest));
        body.push_str("  ");
        body.push_str(path);
        body.push('\n');
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create {}: {err}", parent.display()))?;
    }
    let mut file = File::create(&output).map_err(|err| format!("create {}: {err}", output.display()))?;
    file.write_all(body.as_bytes())
        .map_err(|err| format!("write {}: {err}", output.display()))?;
    println!("files={}", rows.len());
    println!("empty_sha256={EMPTY_SHA}");
    println!("abc_sha256={ABC_SHA}");
    Ok(())
}

fn check_sha256() -> Result<(), String> {
    if hex(&sha256(b"")) != EMPTY_SHA || hex(&sha256(b"abc")) != ABC_SHA {
        return Err("sha256 self-check failed".to_owned());
    }
    Ok(())
}

fn walk(dir: &Path, root: &Path, output_abs: &Path, prefix: &str, rows: &mut Vec<(String, [u8; 32])>) -> Result<(), String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| format!("read {}: {err}", dir.display()))?
        .collect::<Result<Vec<_>, io::Error>>()
        .map_err(|err| format!("read {}: {err}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type().map_err(|err| format!("type {}: {err}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!("symlink refused: {}", path.display()));
        }
        if kind.is_dir() {
            walk(&path, root, output_abs, prefix, rows)?;
            continue;
        }
        if !kind.is_file() {
            return Err(format!("not a regular file: {}", path.display()));
        }
        let abs = absolute(&path)?;
        if abs == output_abs {
            continue;
        }
        let bytes = fs::read(&path).map_err(|err| format!("read {}: {err}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| format!("outside root: {}", path.display()))?;
        let mut shown = if prefix.is_empty() {
            String::from("./")
        } else {
            format!("{prefix}/")
        };
        for (index, part) in relative.components().enumerate() {
            if index > 0 {
                shown.push('/');
            }
            shown.push_str(part.as_os_str().to_str().ok_or_else(|| format!("non-utf8 path: {}", path.display()))?);
        }
        rows.push((shown, sha256(&bytes)));
    }
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = env::current_dir().map_err(|err| format!("cwd: {err}"))?;
    Ok(cwd.join(path))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state = [
        0x6a09_e667_u32,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = u64::try_from(input.len()).expect("len").checked_mul(8).expect("bits");
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut word = [0u32; 64];
        for (index, slot) in word.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *slot = u32::from_be_bytes(chunk[start..start + 4].try_into().expect("word"));
        }
        for index in 16..64 {
            let earlier = word[index - 15];
            let later = word[index - 2];
            let s0 = earlier.rotate_right(7) ^ earlier.rotate_right(18) ^ (earlier >> 3);
            let s1 = later.rotate_right(17) ^ later.rotate_right(19) ^ (later >> 10);
            word[index] = word[index - 16]
                .wrapping_add(s0)
                .wrapping_add(word[index - 7])
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
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(word[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}
