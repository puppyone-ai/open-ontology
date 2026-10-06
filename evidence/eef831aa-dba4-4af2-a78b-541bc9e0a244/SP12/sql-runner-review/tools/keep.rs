//! Copy a file into evidence only when it has no cells or secrets.

use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let source = args.next().expect("source");
    let dest = args.next().expect("dest");
    let secret_path = args.next().expect("secret");
    let mut secret = fs::read(&secret_path).unwrap_or_default();
    if secret.ends_with(b"\n") {
        secret.pop();
    }
    if secret.ends_with(b"\r") {
        secret.pop();
    }
    let bytes = match fs::read(&source) {
        Ok(bytes) => bytes,
        Err(_) => {
            let _ = fs::write(&dest, b"missing\n");
            return ExitCode::from(0);
        }
    };
    let bans: &[&[u8]] = &[
        b"CHANGED-CELL-a244",
        b"PRIVATE KEY",
        b"sealed-left",
        b"sealed-mid",
        b"sealed-right",
        "\u{96ea}".as_bytes(),
        b"beta",
        b"-9223372036854775808",
        b"9223372036854775807",
        b"/evidence/built/gold",
    ];
    let dirty = (!secret.is_empty() && contains(&bytes, &secret))
        || bans.iter().any(|ban| contains(&bytes, ban));
    if dirty {
        let note = format!("withheld\nbytes={}\n", bytes.len());
        if fs::write(&dest, note).is_err() {
            return ExitCode::from(2);
        }
        println!("withheld");
        return ExitCode::from(0);
    }
    if fs::write(&dest, &bytes).is_err() {
        return ExitCode::from(2);
    }
    println!("kept");
    ExitCode::from(0)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
