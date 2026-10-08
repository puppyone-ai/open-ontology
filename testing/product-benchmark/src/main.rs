//! Score one product-benchmark input directory.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use product_benchmark::score::{self, report_json, report_markdown};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(token) => {
            eprintln!("product-benchmark: {token}");
            if token == "usage" {
                ExitCode::from(2)
            } else {
                ExitCode::from(1)
            }
        }
    }
}

fn run() -> Result<u8, &'static str> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or("usage")?;
    if command == "--help" && args.next().is_none() {
        println!("product-benchmark score --freeze <path> --input <dir> --out <dir>");
        return Ok(0);
    }
    if command != "score" {
        return Err("usage");
    }
    let mut freeze = None;
    let mut input = None;
    let mut out = None;
    let rest: Vec<String> = args.collect();
    let mut index = 0;
    while index < rest.len() {
        let flag = rest[index].as_str();
        let value = rest.get(index + 1).ok_or("usage")?;
        match flag {
            "--freeze" => freeze = Some(PathBuf::from(value)),
            "--input" => input = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            _ => return Err("usage"),
        }
        index += 2;
    }
    let freeze = freeze.ok_or("usage")?;
    let input = input.ok_or("usage")?;
    let out = out.ok_or("usage")?;
    let report = score::score(&freeze, &input);
    fs::create_dir_all(&out).map_err(|_| "io")?;
    let body = report_json(&report);
    let markdown = report_markdown(&report);
    fs::write(out.join("report.json"), body).map_err(|_| "io")?;
    fs::write(out.join("report.md"), markdown).map_err(|_| "io")?;
    if report.benchmark_loop_pass { Ok(0) } else { Ok(1) }
}
