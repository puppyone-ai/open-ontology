//! Argument parser for the standalone generator. There is no default seed
//! and no default output directory.

use std::env;
use std::path::PathBuf;

use oo_core::tz::TZDB_VERSION;
use oo_core::{CANONICAL_VALUE_VERSION, TYPE_POLICY_V1};

use crate::attacks::CATEGORIES;
use crate::authz::role_names;
use crate::catalog::{self, header};
use crate::error::{Result, err};
use crate::export::export_sources;
use crate::facts::{DATASET, PROTOCOL, SMELLS};
use crate::generate::{Options, generate};
use crate::pathsafety::Exists;
use crate::plan::{Scale, SmellSet};
use crate::prng::PRNG_VERSION;
use crate::verify::verify_dataset;

pub fn version_text() -> String {
    format!(
        "oo-datagen {version}\nprotocol {PROTOCOL}\nprng {PRNG_VERSION}\ndataset {DATASET}\n{CANONICAL_VALUE_VERSION}\n{TYPE_POLICY_V1}\ntzdb {TZDB_VERSION}\n",
        version = env!("CARGO_PKG_VERSION")
    )
}

pub fn help_text() -> &'static str {
    "oo-datagen — deterministic OO-Logistics xs/s generator\n\
\n\
Commands:\n\
  help\n\
  version\n\
  generate --scale xs|s --seed UINT --output DIR [--scope DIR]\n\
            [--if-exists fail|check|resume] [--smell NAME=off]...\n\
            [--inject-stop-after N]\n\
  verify --dataset DIR\n\
  export-sources --dataset DIR --output DIR [--scope DIR]\n\
  list schema\n\
  list fixtures\n\
\n\
generate writes a new directory. It fails if the directory exists unless\n\
--if-exists check (verify, do not rewrite) or --if-exists resume (delete only\n\
an owned stage, never the destination). Scales m and l are refused.\n\
--inject-stop-after stops after N opened files and removes the owned stage.\n\
export-sources copies source-inputs only.\n"
}

pub fn list_schema() -> String {
    let mut out = String::new();
    for table in catalog::mysql_tables()
        .iter()
        .chain(catalog::postgres_tables())
    {
        let pk = if table.pk.is_empty() {
            "none".to_string()
        } else {
            table.pk.join(",")
        };
        let cols = header(table).join(",");
        out.push_str(&format!(
            "{} {} pk={pk} cols={cols}\n",
            table.dialect, table.name
        ));
    }
    out.push_str(&format!(
        "csv {} pk=none cols={}\n",
        catalog::TXN.name,
        header(&catalog::TXN).join(",")
    ));
    out
}

pub fn list_fixtures() -> String {
    let mut out = String::from("smells\n");
    for name in SMELLS {
        out.push_str(name);
        out.push('\n');
    }
    out.push_str("sql-categories\n");
    for name in CATEGORIES {
        out.push_str(name);
        out.push('\n');
    }
    out.push_str("roles\n");
    for name in role_names() {
        out.push_str(name);
        out.push('\n');
    }
    out
}

pub fn run<I>(args: I) -> Result<i32>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|item| item.as_ref().to_string())
        .collect();
    let Some(command) = args.first().map(String::as_str) else {
        eprintln!("{}", help_text());
        return Ok(2);
    };
    match command {
        "help" | "--help" | "-h" => {
            if args.len() != 1 {
                eprintln!("{}", help_text());
                return Ok(2);
            }
            print!("{}", help_text());
            Ok(0)
        }
        "version" | "--version" | "-V" => {
            if args.len() != 1 {
                eprintln!("{}", help_text());
                return Ok(2);
            }
            print!("{}", version_text());
            Ok(0)
        }
        "generate" => generate_cmd(&args[1..]),
        "verify" => verify_cmd(&args[1..]),
        "export-sources" => export_cmd(&args[1..]),
        "list" => list_cmd(&args[1..]),
        _ => {
            eprintln!("unknown command {command}\n{}", help_text());
            Ok(2)
        }
    }
}

fn generate_cmd(args: &[String]) -> Result<i32> {
    if optional(args, "--scale")?.is_none()
        || optional(args, "--seed")?.is_none()
        || optional(args, "--output")?.is_none()
    {
        eprintln!("{}", help_text());
        return Ok(2);
    }
    let scale = required(args, "--scale")?;
    let seed = required(args, "--seed")?;
    let output = required(args, "--output")?;
    let scope = optional(args, "--scope")?;
    let exists = optional(args, "--if-exists")?.unwrap_or_else(|| "fail".to_string());
    let stop = optional(args, "--inject-stop-after")?;
    let mut smells_off = Vec::new();
    for item in repeated(args, "--smell")? {
        let (name, value) = item
            .split_once('=')
            .ok_or_else(|| err(format!("bad smell flag {item}")))?;
        if value == "off" {
            smells_off.push(name.to_string());
        } else if value != "on" {
            return Err(err(format!("bad smell flag {item}")));
        }
    }
    reject_unknown(
        args,
        &[
            "--scale",
            "--seed",
            "--output",
            "--scope",
            "--if-exists",
            "--inject-stop-after",
            "--smell",
        ],
    )?;
    let scale = Scale::parse(&scale).ok_or_else(|| err(format!("unknown scale {scale}")))?;
    let seed = seed
        .parse::<u64>()
        .map_err(|_| err("seed is not an integer"))?;
    let exists =
        Exists::parse(&exists).ok_or_else(|| err(format!("unknown if-exists {exists}")))?;
    let stop_after_files = match stop {
        Some(value) => Some(
            value
                .parse::<u32>()
                .map_err(|_| err("stop count is not an integer"))?,
        ),
        None => None,
    };
    let scope = match scope {
        Some(value) => PathBuf::from(value),
        None => env::current_dir()?,
    };
    let report = generate(&Options {
        scale,
        seed,
        output: PathBuf::from(output),
        scope,
        exists,
        smells: SmellSet::parse_off(&smells_off)?,
        stop_after_files,
    })?;
    println!("tree_sha256={}", report.tree_sha256);
    println!("scale={}", report.scale);
    println!("seed={}", report.seed);
    println!("partition={:04X}", report.partition);
    Ok(0)
}

fn verify_cmd(args: &[String]) -> Result<i32> {
    let dataset = required(args, "--dataset")?;
    reject_unknown(args, &["--dataset"])?;
    let report = verify_dataset(PathBuf::from(dataset).as_path())?;
    println!("ok tree_sha256={}", report.tree_sha256);
    Ok(0)
}

fn export_cmd(args: &[String]) -> Result<i32> {
    let dataset = required(args, "--dataset")?;
    let output = required(args, "--output")?;
    let scope = optional(args, "--scope")?;
    reject_unknown(args, &["--dataset", "--output", "--scope"])?;
    let scope = match scope {
        Some(value) => PathBuf::from(value),
        None => env::current_dir()?,
    };
    export_sources(
        PathBuf::from(dataset).as_path(),
        PathBuf::from(&output).as_path(),
        &scope,
    )?;
    println!("exported source-inputs only");
    println!("output={output}");
    Ok(0)
}

fn list_cmd(args: &[String]) -> Result<i32> {
    let which = args.first().map(String::as_str).unwrap_or("");
    if args.len() != 1 {
        eprintln!("{}", help_text());
        return Ok(2);
    }
    match which {
        "schema" => print!("{}", list_schema()),
        "fixtures" => print!("{}", list_fixtures()),
        _ => {
            eprintln!("{}", help_text());
            return Ok(2);
        }
    }
    Ok(0)
}

fn required(args: &[String], name: &str) -> Result<String> {
    optional(args, name)?.ok_or_else(|| err(format!("missing {name}")))
}

fn optional(args: &[String], name: &str) -> Result<Option<String>> {
    let found = repeated(args, name)?;
    match found.len() {
        0 => Ok(None),
        1 => Ok(Some(found.into_iter().next().unwrap_or_default())),
        _ => Err(err(format!("repeated {name}"))),
    }
}

fn repeated(args: &[String], name: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let token = &args[index];
        if token == name {
            let value = args
                .get(index + 1)
                .ok_or_else(|| err(format!("missing value for {name}")))?;
            if value.starts_with("--") {
                return Err(err(format!("missing value for {name}")));
            }
            out.push(value.clone());
            index += 2;
            continue;
        }
        if let Some(value) = token.strip_prefix(&format!("{name}=")) {
            if value.is_empty() {
                return Err(err(format!("missing value for {name}")));
            }
            out.push(value.to_string());
        }
        index += 1;
    }
    Ok(out)
}

fn reject_unknown(args: &[String], known: &[&str]) -> Result<()> {
    let mut index = 0;
    while index < args.len() {
        let token = &args[index];
        let name = token.split('=').next().unwrap_or(token);
        if name.starts_with("--") {
            if !known.contains(&name) {
                return Err(err(format!("unknown argument {name}")));
            }
            if token.contains('=') {
                if token.split_once('=').is_some_and(|(_, value)| value.is_empty()) {
                    return Err(err(format!("missing value for {name}")));
                }
                index += 1;
                continue;
            }
            let value = args
                .get(index + 1)
                .ok_or_else(|| err(format!("missing value for {name}")))?;
            if value.starts_with("--") {
                return Err(err(format!("missing value for {name}")));
            }
            index += 2;
            continue;
        }
        return Err(err(format!("unsupported positional argument {token}")));
    }
    Ok(())
}
