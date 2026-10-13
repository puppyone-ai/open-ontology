//! Commands for the logistics fixture and the closed acceptance card.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use logistics_product_loop::{
    canonical_event_line, canonical_hash, canonical_hub_line, canonical_shipment_line, judge, parse, sha256_hex, verdict_text,
    write_fixture, write_gold, EVENT_ROWS, HUB_ROWS, SHIPMENT_ROWS,
};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<u8, String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map(String::as_str).unwrap_or("");
    match command {
        "generate" => {
            let dir = required(&args, "--out")?;
            write_fixture(&PathBuf::from(dir))?;
            println!("generated hubs={HUB_ROWS} shipments={SHIPMENT_ROWS} tracking_events={EVENT_ROWS}");
            Ok(0)
        }
        "gold" => {
            let dir = required(&args, "--out")?;
            write_gold(&PathBuf::from(dir))?;
            println!("gold questions=6 oracle_separated=true");
            Ok(0)
        }
        "card" => {
            let out = required(&args, "--out")?;
            let parsed = match optional(&args, "--observation")? {
                Some(path) => Some(parse(&fs::read_to_string(path).map_err(|error| error.to_string())?)?),
                None => None,
            };
            let verdict = judge(parsed.as_ref());
            let dir = PathBuf::from(out);
            fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
            fs::write(dir.join("verdict.json"), verdict_text(&verdict)).map_err(|error| error.to_string())?;
            println!("{}", verdict_text(&verdict));
            u8::try_from(verdict.exit_code).map_err(|_| "exit".to_owned())
        }
        "dump-canonical" => {
            let table = args.get(1).ok_or_else(|| "missing table".to_owned())?;
            let rows = match table.as_str() {
                "hubs" => HUB_ROWS,
                "shipments" => SHIPMENT_ROWS,
                "tracking_events" => EVENT_ROWS,
                _ => return Err("unknown table".to_owned()),
            };
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            for id in 1..=rows {
                let line = match table.as_str() {
                    "hubs" => canonical_hub_line(id),
                    "shipments" => canonical_shipment_line(id),
                    _ => canonical_event_line(id),
                };
                writeln!(handle, "{line}").map_err(|error| error.to_string())?;
            }
            Ok(0)
        }
        "hash-table" => {
            let table = args.get(1).ok_or_else(|| "missing table".to_owned())?;
            println!("{}", canonical_hash(table)?);
            Ok(0)
        }
        "hash-stdin" => {
            let expect = required(&args, "--expect")?;
            let mut bytes = Vec::new();
            io::stdin().read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            let got = sha256_hex(&bytes);
            if got != expect {
                return Err(format!("hash mismatch {got}"));
            }
            println!("{got}");
            Ok(0)
        }
        _ => Err("usage: logistics-product-loop <generate|gold|card|dump-canonical|hash-table|hash-stdin>".to_owned()),
    }
}

fn required<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    optional(args, name)?.ok_or_else(|| format!("missing {name}"))
}

fn optional<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
    let mut index = 0;
    while index < args.len() {
        if args[index] == name {
            return args.get(index + 1).map(String::as_str).map(Some).ok_or_else(|| format!("missing value for {name}"));
        }
        index += 1;
    }
    Ok(None)
}
