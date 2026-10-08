//! Operator CLI. `authenticated` is false until a parent identity service exists.

#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use curated_git_apply::{OperatorRegistry, apply_curated_git, cli_deadline, find_git};

const USAGE: &str = "\
curated-git-apply: usage: curated-git-apply apply --root <registered-checkout>
curated-git-apply:   apply    one oo.curated-git-apply/v1 document on stdin
curated-git-apply: stdout is the receipt and has no added newline
curated-git-apply: this command does not authenticate, publish, grant, push, fetch, or run hooks
curated-git-apply: authenticated is false and parent authentication is required
";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(phase) = args.next() else {
        print_usage();
        return ExitCode::from(2);
    };
    if phase != "apply" {
        print_usage();
        return ExitCode::from(2);
    }
    let Some(flag) = args.next() else {
        eprintln!("curated-git-apply: the phase takes --root and no other arguments");
        return ExitCode::from(2);
    };
    if flag != "--root" {
        eprintln!("curated-git-apply: the phase takes --root and no other arguments");
        return ExitCode::from(2);
    }
    let Some(root) = args.next() else {
        eprintln!("curated-git-apply: the phase takes --root and no other arguments");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("curated-git-apply: the phase takes --root and no other arguments");
        return ExitCode::from(2);
    }
    let Some(git_binary) = std::env::var_os("PATH").and_then(|path| {
        find_git(&path.to_string_lossy()).or_else(|| {
            path.to_str().and_then(find_git)
        })
    }) else {
        eprintln!("curated-git-apply: git.spawn git could not be started");
        return ExitCode::from(1);
    };
    match read_stdin() {
        Err(error) => {
            eprintln!("curated-git-apply: {} {}", error.code, error.message);
            ExitCode::from(1)
        }
        Ok(input) => {
            let registry = OperatorRegistry {
                project_root: PathBuf::from(root),
                git_binary,
            };
            match apply_curated_git(&registry, &input, cli_deadline()) {
                Ok(receipt) => {
                    let mut out = io::stdout().lock();
                    if out.write_all(receipt.as_bytes()).is_err() {
                        eprintln!("curated-git-apply: io.write the receipt could not be written");
                        return ExitCode::from(1);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("curated-git-apply: {} {}", error.code, error.message);
                    ExitCode::from(1)
                }
            }
        }
    }
}

fn print_usage() {
    let _ = io::stderr().write_all(USAGE.as_bytes());
}

fn read_stdin() -> Result<String, curated_git_apply::CuratedGitError> {
    let mut buffer = Vec::new();
    let mut taken = 0usize;
    let mut chunk = [0u8; 8192];
    let mut stdin = io::stdin().lock();
    loop {
        let read = stdin.read(&mut chunk).map_err(|_| {
            curated_git_apply::CuratedGitError::new("io.read", "the request could not be read")
        })?;
        if read == 0 {
            break;
        }
        taken += read;
        if taken > 2_097_152 {
            return Err(curated_git_apply::CuratedGitError::new(
                "json.budget.bytes",
                "document exceeds the byte budget",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(buffer).map_err(|_| {
        curated_git_apply::CuratedGitError::new("json.syntax", "the request is not JSON")
    })
}
