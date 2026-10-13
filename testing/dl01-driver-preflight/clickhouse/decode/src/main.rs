mod decimal;
mod extract;
mod fail;
mod judge;
mod timefmt;

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use fail::{fail, Gate};
use serde_json::Value;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Gate<ExitCode> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(|| fail("missing command"))?;
    let options = parse_options(args.collect())?;
    let out = required(&options, "out")?;
    let verdict = match command.as_str() {
        "compare" => {
            let schema = required(&options, "schema")?;
            if schema != "readings" {
                return Err(fail("compare schema must be readings"));
            }
            judge::compare_readings(Path::new(&required(&options, "ipc")?), Path::new(&required(&options, "gold")?))?
        }
        "classify-bytes" => judge::classify_bytes(
            Path::new(&required(&options, "ipc")?),
            &required(&options, "column")?,
            &required(&options, "expect-hex")?,
            flag(&options, "utf8-constraint"),
        )?,
        "judge-retirees" => {
            judge::judge_retirees(Path::new(&required(&options, "ipc")?), Path::new(&required(&options, "mutations")?))?
        }
        other => return Err(fail(format!("unknown command {other}"))),
    };
    write_json(Path::new(&out), &verdict)?;
    if verdict.get("passed").and_then(Value::as_bool) == Some(true) {
        Ok(ExitCode::from(0))
    } else {
        Ok(ExitCode::from(2))
    }
}

fn parse_options(args: Vec<String>) -> Gate<HashMap<String, Vec<String>>> {
    let mut options: HashMap<String, Vec<String>> = HashMap::new();
    let mut index = 0;
    while index < args.len() {
        let item = &args[index];
        let Some(key) = item.strip_prefix("--") else {
            return Err(fail(format!("unexpected argument {item}")));
        };
        if key == "utf8-constraint" {
            options.entry(key.to_string()).or_default().push("true".into());
            index += 1;
            continue;
        }
        let Some(value) = args.get(index + 1) else {
            return Err(fail(format!("missing value for --{key}")));
        };
        if value.starts_with("--") {
            return Err(fail(format!("missing value for --{key}")));
        }
        options.entry(key.to_string()).or_default().push(value.clone());
        index += 2;
    }
    Ok(options)
}

fn required(options: &HashMap<String, Vec<String>>, key: &str) -> Gate<String> {
    let values = options.get(key).ok_or_else(|| fail(format!("missing --{key}")))?;
    if values.len() != 1 {
        return Err(fail(format!("--{key} must be given once")));
    }
    Ok(values[0].clone())
}

fn flag(options: &HashMap<String, Vec<String>>, key: &str) -> bool {
    options.contains_key(key)
}

fn write_json(path: &Path, value: &Value) -> Gate<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| fail(format!("mkdir: {error}")))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|error| fail(format!("json: {error}")))?;
    let shown = path.display().to_string();
    fs::write(path, text + "\n").map_err(|error| fail(format!("write {shown}: {error}")))
}
