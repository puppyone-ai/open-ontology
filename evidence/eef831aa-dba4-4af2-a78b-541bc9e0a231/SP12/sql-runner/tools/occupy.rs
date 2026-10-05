//! Create the trial audit file before query-pg does.
//!
//! query-pg refuses create_new when the audit path already exists. That is a
//! real missing-completion exit, not an edited stdout.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

fn main() -> ExitCode {
    let trial = PathBuf::from(env::args().nth(1).expect("trial dir"));
    let audit = trial.join("audit.jsonl");
    let parent = trial.parent().map(Path::to_path_buf);
    let start = Instant::now();
    loop {
        if trial.is_dir() {
            match OpenOptions::new().write(true).create_new(true).open(&audit) {
                Ok(mut file) => {
                    if file.write_all(b"keep\n").is_err() {
                        return ExitCode::from(2);
                    }
                    return ExitCode::from(0);
                }
                Err(_) => {
                    if audit.exists() {
                        return ExitCode::from(3);
                    }
                }
            }
        }
        if start.elapsed() > Duration::from_secs(180) {
            eprintln!("occupy: trial dir did not appear");
            return ExitCode::from(2);
        }
        let case_dir_ready = parent.as_ref().is_some_and(|path| path.is_dir());
        if !case_dir_ready && start.elapsed() > Duration::from_millis(500) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
