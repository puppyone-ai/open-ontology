//! Presence scan for review outputs. Writes status words only.
//! It does not print secret bytes.

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

const TOKENS: [&[u8]; 4] = [
    b"tok-north-208",
    b"tok-south-208",
    b"tok-east-208",
    b"env-not-used-a208",
];

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: secret-scan <admin> <login> <out> <file>...");
        return ExitCode::from(2);
    }
    let admin_bytes = read_quiet(&args[0]);
    let login_bytes = read_quiet(&args[1]);
    let admin = trim(&admin_bytes);
    let login = trim(&login_bytes);
    if admin.len() < 16 || login.len() < 16 {
        eprintln!("secret files are unusable");
        return ExitCode::from(2);
    }
    let mut passwords = "clean";
    let mut private_keys = "clean";
    let mut oracle_physical = "clean";
    let mut banned = "clean";
    let mut missing = "none";
    for path in &args[3..] {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => {
                missing = "yes";
                continue;
            }
        };
        if contains(&bytes, admin) || contains(&bytes, login) {
            passwords = "leak";
        }
        if contains(&bytes, b"BEGIN PRIVATE KEY") || contains(&bytes, b"BEGIN RSA PRIVATE KEY") {
            private_keys = "leak";
        }
        let name = Path::new(path)
            .file_name()
            .and_then(|item| item.to_str())
            .unwrap_or("");
        if name == "oracle.txt" || name == "cli-oracle.txt" {
            if contains(&bytes, b"pt_") || contains(&bytes, b"pc_") || contains(&bytes, b"postgres://") {
                oracle_physical = "leak";
            }
            for token in TOKENS {
                if contains(&bytes, token) {
                    banned = "leak";
                }
            }
        }
    }
    let mut out = match fs::File::create(&args[2]) {
        Ok(file) => file,
        Err(_) => {
            eprintln!("scan output could not be created");
            return ExitCode::from(1);
        }
    };
    let _ = writeln!(out, "passwords={passwords}");
    let _ = writeln!(out, "private_keys={private_keys}");
    let _ = writeln!(out, "oracle_physical={oracle_physical}");
    let _ = writeln!(out, "banned_values={banned}");
    let _ = writeln!(out, "missing={missing}");
    if passwords == "leak" || private_keys == "leak" || oracle_physical == "leak" || banned == "leak" {
        ExitCode::from(3)
    } else {
        ExitCode::SUCCESS
    }
}

fn read_quiet(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_default()
}

fn trim(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b'\n' || bytes[end - 1] == b'\r') {
        end -= 1;
    }
    &bytes[..end]
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
