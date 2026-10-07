#![forbid(unsafe_code)]

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use selfbuild_rules::{canonical_json, produce, score, Fail, MAX_INPUT_BYTES};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        return usage();
    };
    let rest: Vec<String> = args.collect();
    match command.as_str() {
        "produce" => match produce_cli(&rest) {
            Ok(value) => emit(&value),
            Err(err) => refuse(err.code),
        },
        "score" => match score_cli(&rest) {
            Ok(value) => {
                let matched = value.get("matched").and_then(|item| item.as_bool()).unwrap_or(false);
                let code = emit(&value);
                if code != ExitCode::SUCCESS {
                    return code;
                }
                if matched {
                    ExitCode::SUCCESS
                } else {
                    eprintln!("selfbuild-rules: score.mismatch");
                    ExitCode::from(1)
                }
            }
            Err(err) => refuse(err.code),
        },
        _ => usage(),
    }
}

fn produce_cli(args: &[String]) -> Result<serde_json::Value, Fail> {
    let flags = pairs(args)?;
    let catalog = flag(&flags, "--catalog")?;
    let profile = flag(&flags, "--profile")?;
    let cards = flag(&flags, "--cards")?;
    let auto = flag(&flags, "--auto-accept")?;
    produce(&read_capped(Path::new(catalog))?, &read_capped(Path::new(profile))?, &read_capped(Path::new(cards))?, auto)
}

fn score_cli(args: &[String]) -> Result<serde_json::Value, Fail> {
    let flags = pairs(args)?;
    let truth = flag(&flags, "--truth")?;
    let actual = flag(&flags, "--actual")?;
    score(&read_capped(Path::new(truth))?, &read_capped(Path::new(actual))?)
}

fn pairs(args: &[String]) -> Result<Vec<(&str, &str)>, Fail> {
    if !args.len().is_multiple_of(2) {
        return Err(Fail::new("usage"));
    }
    let mut out = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let key = args[index].as_str();
        if !key.starts_with("--") || out.iter().any(|(seen, _)| *seen == key) {
            return Err(Fail::new("usage"));
        }
        out.push((key, args[index + 1].as_str()));
        index += 2;
    }
    Ok(out)
}

fn flag<'a>(flags: &[(&str, &'a str)], name: &str) -> Result<&'a str, Fail> {
    flags.iter().find(|(key, _)| *key == name).map(|(_, value)| *value).ok_or(Fail::new("usage"))
}

fn read_capped(path: &Path) -> Result<Vec<u8>, Fail> {
    let mut file = File::open(path).map_err(|_| Fail::new("io"))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = file.read(&mut chunk).map_err(|_| Fail::new("io"))?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > MAX_INPUT_BYTES {
            return Err(Fail::new("bound.input"));
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    Ok(buf)
}

fn emit(value: &serde_json::Value) -> ExitCode {
    let text = canonical_json(value);
    let mut out = io::stdout().lock();
    match writeln!(out, "{text}") {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => refuse("io"),
    }
}

fn refuse(code: &str) -> ExitCode {
    eprintln!("selfbuild-rules: {code}");
    ExitCode::from(if code == "usage" { 2 } else { 1 })
}

fn usage() -> ExitCode {
    refuse("usage")
}
