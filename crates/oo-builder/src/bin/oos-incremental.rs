use std::io::{Read, Write};
use std::process::ExitCode;

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 {
        return Err("usage: oos-incremental CEILINGS_JSON < REQUEST_JSON".into());
    }
    let ceilings: oo_builder::RolePackCeilings =
        serde_json::from_str(&std::fs::read_to_string(&args[0]).map_err(|_| "ceilings_read")?)
            .map_err(|_| "ceilings_invalid")?;
    let mut input = String::new();
    std::io::stdin()
        .take(ceilings.json_scan.max_bytes as u64 + 1)
        .read_to_string(&mut input)
        .map_err(|_| "request_read")?;
    let result = oo_builder::compile_library_incremental(&input, &ceilings)?;
    std::io::stdout()
        .write_all(result.as_bytes())
        .map_err(|_| "result_write")?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oos-incremental: {error}");
            ExitCode::from(1)
        }
    }
}
