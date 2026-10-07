//! Dispatch for the public fixture driver.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use pg_e2e::run;
use pg_e2e::Fail;

fn main() -> ExitCode {
    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(detail) => {
            eprintln!("pg-e2e: {detail}");
            if detail == "usage" {
                ExitCode::from(2)
            } else {
                ExitCode::from(1)
            }
        }
    }
}

fn dispatch() -> Result<(), Fail> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(|| fail("usage"))?;
    let rest: Vec<String> = args.collect();
    match command.as_str() {
        "emit" => run::emit(Path::new(&one(&rest)?)),
        "inspect-config" => {
            let parts = four(&rest)?;
            write_text(&parts[3], run::inspect_config(Path::new(&parts[0]), Path::new(&parts[1]), Path::new(&parts[2]))?)
        }
        "bind" => {
            let parts = two(&rest)?;
            run::bind(Path::new(&parts[0]), Path::new(&parts[1]))
        }
        "apply-config" => {
            let parts = four(&rest)?;
            write_text(&parts[3], run::apply_config(Path::new(&parts[0]), Path::new(&parts[1]), Path::new(&parts[2]))?)
        }
        "check-inspect" => run::check_inspect(Path::new(&one(&rest)?)),
        "check-apply" => {
            let parts = two(&rest)?;
            run::check_apply(Path::new(&parts[0]), Path::new(&parts[1]))
        }
        "copy-sql" => {
            let parts = three(&rest)?;
            write_text(&parts[2], run::copy_sql(Path::new(&parts[0]), &parts[1])?)
        }
        "check-copy" => {
            let parts = two(&rest)?;
            run::check_copy(&parts[0], Path::new(&parts[1]))
        }
        "fresh-request" => {
            let parts = five(&rest)?;
            write_text(
                &parts[4],
                run::fresh_request(Path::new(&parts[0]), Path::new(&parts[1]), Path::new(&parts[2]), Path::new(&parts[3]))?,
            )
        }
        "probe-request" => {
            let parts = five(&rest)?;
            write_text(
                &parts[4],
                run::probe_request(Path::new(&parts[0]), Path::new(&parts[1]), Path::new(&parts[2]), Path::new(&parts[3]))?,
            )
        }
        "check-fresh" => {
            let parts = three(&rest)?;
            run::check_fresh(Path::new(&parts[0]), Path::new(&parts[1]), Path::new(&parts[2]))
        }
        "note-probe" => {
            let parts = three(&rest)?;
            write_text(&parts[2], run::note_probe(Path::new(&parts[0]), Path::new(&parts[1]))?)
        }
        "record-measurements" => {
            let parts = three(&rest)?;
            run::record_measurements(Path::new(&parts[0]), Path::new(&parts[1]), Path::new(&parts[2]))
        }
        "pack-digest" => {
            let digest = run::pack_digest(Path::new(&one(&rest)?))?;
            println!("{digest}");
            Ok(())
        }
        _ => Err(fail("usage")),
    }
}

fn one(args: &[String]) -> Result<String, Fail> {
    exact(args, 1)
}

fn two(args: &[String]) -> Result<[String; 2], Fail> {
    Ok([exact(args, 2)?, args[1].clone()])
}

fn three(args: &[String]) -> Result<[String; 3], Fail> {
    Ok([exact(args, 3)?, args[1].clone(), args[2].clone()])
}

fn four(args: &[String]) -> Result<[String; 4], Fail> {
    Ok([exact(args, 4)?, args[1].clone(), args[2].clone(), args[3].clone()])
}

fn five(args: &[String]) -> Result<[String; 5], Fail> {
    Ok([exact(args, 5)?, args[1].clone(), args[2].clone(), args[3].clone(), args[4].clone()])
}

fn exact(args: &[String], count: usize) -> Result<String, Fail> {
    if args.len() != count {
        return Err(fail("usage"));
    }
    Ok(args[0].clone())
}

fn write_text(path: &str, text: String) -> Result<(), Fail> {
    let out = Path::new(path);
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|_| fail("out dir"))?;
    }
    fs::write(out, text).map_err(|_| fail("out write"))
}

fn fail(detail: &str) -> Fail {
    detail.to_owned()
}
