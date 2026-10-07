//! Scan a directory for a secret file's exact bytes. Does not print the secret.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(secret_path) = args.next() else {
        eprintln!("usage: leak_scan <secret> <root>");
        return ExitCode::from(2);
    };
    let Some(root) = args.next() else {
        eprintln!("usage: leak_scan <secret> <root>");
        return ExitCode::from(2);
    };
    let secret = match fs::read(&secret_path) {
        Ok(bytes) => trim_newline(bytes),
        Err(err) => {
            eprintln!("read secret file: {err}");
            return ExitCode::from(1);
        }
    };
    if secret.is_empty() {
        eprintln!("secret file is empty");
        return ExitCode::from(1);
    }
    let mut hits = 0_u32;
    if scan(Path::new(&root), &secret, &mut hits).is_err() {
        eprintln!("scan failed");
        return ExitCode::from(1);
    }
    println!("leak={hits}");
    if hits == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn trim_newline(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    bytes
}

fn scan(dir: &Path, secret: &[u8], hits: &mut u32) -> Result<(), ()> {
    let entries = fs::read_dir(dir).map_err(|_| ())?;
    for entry in entries {
        let entry = entry.map_err(|_| ())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|_| ())?;
        if kind.is_symlink() {
            return Err(());
        }
        if kind.is_dir() {
            scan(&path, secret, hits)?;
        } else if kind.is_file() {
            let bytes = fs::read(&path).map_err(|_| ())?;
            if bytes.windows(secret.len()).any(|window| window == secret) {
                *hits += 1;
                println!("leak_path={}", path.display());
            }
        }
    }
    Ok(())
}
