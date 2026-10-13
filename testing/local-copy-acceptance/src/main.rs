#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use local_copy_acceptance::{judge, parse_case, run_producer, verdict_json, Fail};

fn main() -> ExitCode {
    match dispatch() {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            let _ = writeln!(io::stderr(), "{} {}", err.code, err.detail);
            ExitCode::from(err.exit_code())
        }
    }
}

fn dispatch() -> Result<u8, Fail> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(Fail {
        code: "usage",
        detail: "judge or run",
    })?;
    match command.as_str() {
        "judge" => {
            let case_path = args.next().ok_or(Fail {
                code: "usage",
                detail: "judge <case> <verdict>",
            })?;
            let verdict_path = args.next().ok_or(Fail {
                code: "usage",
                detail: "judge <case> <verdict>",
            })?;
            if args.next().is_some() {
                return Err(Fail {
                    code: "usage",
                    detail: "judge <case> <verdict>",
                });
            }
            let text = read_case(Path::new(&case_path))?;
            let case = parse_case(&text)?;
            let verdict = judge(&case);
            write_text(Path::new(&verdict_path), &verdict_json(&verdict))?;
            let word = if verdict.mp01_signed {
                "signed"
            } else {
                "withheld"
            };
            println!("{word} {} false", verdict.sign_blockers.len());
            Ok(0)
        }
        "run" => {
            let binary = args.next().ok_or(Fail {
                code: "usage",
                detail: "run <binary> <case> <verdict>",
            })?;
            let case_path = args.next().ok_or(Fail {
                code: "usage",
                detail: "run <binary> <case> <verdict>",
            })?;
            let verdict_path = args.next().ok_or(Fail {
                code: "usage",
                detail: "run <binary> <case> <verdict>",
            })?;
            if args.next().is_some() {
                return Err(Fail {
                    code: "usage",
                    detail: "run <binary> <case> <verdict>",
                });
            }
            let text = read_case(Path::new(&case_path))?;
            let case = parse_case(&text)?;
            let report = run_producer(Path::new(&binary), &case)?;
            let body = format!(
                "{{\n  \"executed\": {},\n  \"exit_code\": {},\n  \"reason\": \"{}\",\n  \"mp01_signed\": false,\n  \"customer_complete\": false,\n  \"issued_stop\": false,\n  \"runner_source_sockets\": 0\n}}\n",
                if report.executed { "true" } else { "false" },
                report.exit_code.map(|code| code.to_string()).unwrap_or_else(|| "null".to_string()),
                report.reason
            );
            write_text(Path::new(&verdict_path), &body)?;
            println!(
                "{} {} false",
                if report.executed {
                    "executed"
                } else {
                    "not_executed"
                },
                report.reason
            );
            Ok(if report.executed { 5 } else { 4 })
        }
        _ => Err(Fail {
            code: "usage",
            detail: "judge or run",
        }),
    }
}

fn read_case(path: &Path) -> Result<String, Fail> {
    let meta = fs::symlink_metadata(path).map_err(|_| Fail {
        code: "config",
        detail: "case path",
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(Fail {
            code: "config",
            detail: "case path",
        });
    }
    if meta.len() > 64 * 1024 * 1024 {
        return Err(Fail {
            code: "bound",
            detail: "case file",
        });
    }
    let bytes = fs::read(path).map_err(|_| Fail {
        code: "config",
        detail: "case path",
    })?;
    String::from_utf8(bytes).map_err(|_| Fail {
        code: "malformed",
        detail: "utf8",
    })
}

fn write_text(path: &Path, text: &str) -> Result<(), Fail> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(Fail {
                code: "config",
                detail: "verdict path",
            });
        }
    }
    fs::write(path, text).map_err(|_| Fail {
        code: "config",
        detail: "verdict path",
    })
}
