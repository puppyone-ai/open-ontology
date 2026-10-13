#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("starrocks-decode: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut out = None;
    let mut inputs = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                out = Some(PathBuf::from(args.get(index).ok_or("missing --out")?));
            }
            other if other.starts_with('-') => return Err(other.to_string()),
            other => inputs.push(PathBuf::from(other)),
        }
        index += 1;
    }
    let out = out.ok_or("--out")?;
    if inputs.is_empty() {
        return Err("missing ipc input".to_string());
    }
    fs::create_dir_all(&out).map_err(|err| err.to_string())?;
    for input in inputs {
        let value = starrocks_decode::decode_file(&input)?;
        let stem = input.file_stem().and_then(|item| item.to_str()).ok_or("stem")?;
        let path = out.join(format!("{stem}.json"));
        let mut bytes = serde_json::to_vec_pretty(&value).map_err(|err| err.to_string())?;
        bytes.push(b'\n');
        fs::write(path, bytes).map_err(|err| err.to_string())?;
    }
    Ok(())
}
