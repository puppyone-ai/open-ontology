//! Frozen publication tail from d20944017bda07ae8a7e95c8acfcfaebb829aa4a.
//!
//! `readback_and_rename` treats `symlink_metadata` NotFound as absence, then
//! calls `std::fs::rename`. This program writes the sentinel in that gap.
//! Exit 1 means the sentinel bytes were replaced. Exit 0 would mean they
//! survived. The product binary has no pause between those two calls, so this
//! file executes the same two operations with the gap made visible.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::process::ExitCode;

const SENTINEL: &[u8] = b"SENTINEL-a221-gap";
const BUNDLE: &[u8] = b"BUNDLE-a221-gap";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            let line = format!("race_red: {err}\n");
            let _ = fs::write("/evidence/race-result.txt", &line);
            let _ = io::stderr().write_all(line.as_bytes());
            ExitCode::from(2)
        }
    }
}

fn run() -> io::Result<ExitCode> {
    let dir = std::env::temp_dir().join(format!("oo-a221-race-red-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir)?;
    let staging = dir.join("out.json.role-pack-bundle-staging");
    let dest = dir.join("out.json");
    let neighbor = dir.join("neighbor.txt");
    fs::write(&staging, BUNDLE)?;
    fs::write(&neighbor, b"KEEP-NEIGHBOR")?;

    let absent = match fs::symlink_metadata(&dest) {
        Ok(_) => false,
        Err(err) if err.kind() == ErrorKind::NotFound => true,
        Err(err) => return Err(err),
    };
    if !absent {
        return finish(&dest, &staging, &neighbor, "present_before_gap", 2);
    }
    fs::write(&dest, SENTINEL)?;
    fs::rename(&staging, &dest)?;
    let got = fs::read(&dest)?;
    let (label, code) = if got == SENTINEL {
        ("preserved", 0)
    } else if got == BUNDLE {
        ("overwritten", 1)
    } else {
        ("unexpected", 2)
    };
    finish(&dest, &staging, &neighbor, label, code)
}

fn finish(
    dest: &std::path::Path,
    staging: &std::path::Path,
    neighbor: &std::path::Path,
    label: &str,
    code: u8,
) -> io::Result<ExitCode> {
    let dest_bytes = fs::read(dest).unwrap_or_default();
    let neighbor_bytes = fs::read(neighbor).unwrap_or_default();
    let body = format!(
        "strategy=symlink_metadata_then_rename\ngap=sentinel_written_after_absence\nresult={label}\ndest_bytes={}\nstaging_exists={}\nneighbor_bytes={}\n",
        String::from_utf8_lossy(&dest_bytes),
        staging.exists(),
        String::from_utf8_lossy(&neighbor_bytes),
    );
    fs::write("/evidence/race-result.txt", &body)?;
    io::stderr().write_all(body.as_bytes())?;
    Ok(ExitCode::from(code))
}
