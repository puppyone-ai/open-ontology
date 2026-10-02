//! Command flags. Parsing does not open files, sockets, or environment credentials.

use crate::outcome::Failure;
use crate::{QUERY_MAX_ROWS, QUERY_TIMEOUT_MAX_MS};

const ARG_MAX_BYTES: usize = 65_536;
const ARG_MAX_COUNT: usize = 32;
const GATEWAY_MAX_BYTES: usize = 2_048;
const PATH_MAX_BYTES: usize = 4_096;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Production,
    LocalDevelopment,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    External,
    Sandbox,
}

pub struct QueryArgs {
    pub json: bool,
    pub gateway: Option<String>,
    pub transport: Option<Transport>,
    pub auth: Option<Auth>,
    pub token_file: Option<String>,
    pub token_stdin: bool,
    pub ca_file: Option<String>,
    pub max_rows: Option<u32>,
    pub timeout_ms: Option<u32>,
    pub sql: Option<String>,
    pub sql_file: Option<String>,
}

pub enum Invocation {
    Help(String),
    Unsupported,
    Query(QueryArgs),
}

pub fn parse() -> Result<Invocation, Failure> {
    let args = os_args()?;
    if args.is_empty() {
        return Err(Failure::usage("command is missing"));
    }
    let command = &args[0];
    if command == "--help" || command == "-h" {
        return Ok(Invocation::Help(top_help()));
    }
    if command.starts_with('-') {
        return Err(Failure::usage("command is missing"));
    }
    if command != "query" {
        if args.iter().any(|arg| arg == "--help" || arg == "-h") {
            return Ok(Invocation::Help(unsupported_help(command)));
        }
        return Ok(Invocation::Unsupported);
    }
    parse_query(&args[1..])
}

fn os_args() -> Result<Vec<String>, Failure> {
    let mut out = Vec::new();
    for arg in std::env::args_os().skip(1) {
        if out.len() == ARG_MAX_COUNT {
            return Err(Failure::usage("too many arguments"));
        }
        let Some(text) = arg.to_str() else {
            return Err(Failure::usage("argument is not utf-8"));
        };
        if text.len() > ARG_MAX_BYTES {
            return Err(Failure::usage("argument exceeds the byte budget"));
        }
        out.push(text.to_owned());
    }
    Ok(out)
}

fn parse_query(args: &[String]) -> Result<Invocation, Failure> {
    if wants_help(args) {
        return Ok(Invocation::Help(query_help()));
    }
    let mut json = false;
    let mut gateway = None;
    let mut transport = None;
    let mut auth = None;
    let mut token_file = None;
    let mut token_stdin = false;
    let mut ca_file = None;
    let mut max_rows = None;
    let mut timeout_ms = None;
    let mut sql_file = None;
    let mut positional = Vec::new();
    let mut ended = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if !ended && arg == "--" {
            ended = true;
            index += 1;
            continue;
        }
        if !ended && arg.starts_with('-') {
            if arg.contains('=') {
                return Err(Failure::usage("flags do not take equals values"));
            }
            match arg {
                "--json" => set_flag(&mut json)?,
                "--token-stdin" => set_flag(&mut token_stdin)?,
                "--gateway" => {
                    let value = take_text(args, &mut index, "gateway", GATEWAY_MAX_BYTES)?;
                    once(&mut gateway, value)?;
                }
                "--transport" => {
                    let value = take_text(args, &mut index, "transport", 32)?;
                    once(&mut transport, parse_transport(&value)?)?;
                }
                "--auth" => {
                    let value = take_text(args, &mut index, "auth", 32)?;
                    once(&mut auth, parse_auth(&value)?)?;
                }
                "--token-file" => {
                    let value = take_text(args, &mut index, "token-file", PATH_MAX_BYTES)?;
                    once(&mut token_file, value)?;
                }
                "--ca-file" => {
                    let value = take_text(args, &mut index, "ca-file", PATH_MAX_BYTES)?;
                    once(&mut ca_file, value)?;
                }
                "--max-rows" => {
                    let value = take_number(args, &mut index, "max-rows", 1, QUERY_MAX_ROWS)?;
                    once(&mut max_rows, value)?;
                }
                "--timeout-ms" => {
                    let value =
                        take_number(args, &mut index, "timeout-ms", 1, QUERY_TIMEOUT_MAX_MS)?;
                    once(&mut timeout_ms, value)?;
                }
                "-f" => {
                    let value = take_sql_path(args, &mut index)?;
                    once(&mut sql_file, value)?;
                }
                _ => return Err(unknown_flag(arg)),
            }
        } else {
            positional.push(args[index].clone());
        }
        index += 1;
    }
    if let Some(path) = &sql_file
        && path == "-"
    {
        return Err(Failure::usage("sql stdin is not supported"));
    }
    if positional.len() > 1 {
        return Err(Failure::usage("extra argument"));
    }
    if positional.len() == 1 && sql_file.is_some() {
        return Err(Failure::usage(
            "positional sql and -f are mutually exclusive",
        ));
    }
    if token_file.is_some() && token_stdin {
        return Err(Failure::usage(
            "token-file and token-stdin are mutually exclusive",
        ));
    }
    let sql = match positional.first() {
        Some(text) if text.is_empty() => return Err(Failure::usage("sql is missing")),
        Some(text) => Some(text.clone()),
        None => None,
    };
    if sql.is_none() && sql_file.is_none() {
        return Err(Failure::usage("sql is missing"));
    }
    Ok(Invocation::Query(QueryArgs {
        json,
        gateway,
        transport,
        auth,
        token_file,
        token_stdin,
        ca_file,
        max_rows,
        timeout_ms,
        sql,
        sql_file,
    }))
}

fn wants_help(args: &[String]) -> bool {
    let mut ended = false;
    for arg in args {
        if !ended && arg == "--" {
            ended = true;
            continue;
        }
        if !ended && (arg == "--help" || arg == "-h") {
            return true;
        }
    }
    false
}

fn once<T>(slot: &mut Option<T>, value: T) -> Result<(), Failure> {
    if slot.is_some() {
        return Err(Failure::usage("duplicate flag"));
    }
    *slot = Some(value);
    Ok(())
}

fn set_flag(slot: &mut bool) -> Result<(), Failure> {
    if *slot {
        return Err(Failure::usage("duplicate flag"));
    }
    *slot = true;
    Ok(())
}

fn take_text(
    args: &[String],
    index: &mut usize,
    name: &str,
    max_bytes: usize,
) -> Result<String, Failure> {
    let Some(value) = args.get(*index + 1) else {
        return Err(Failure::usage("missing flag value"));
    };
    if value.starts_with('-') {
        return Err(Failure::usage("missing flag value"));
    }
    if value.is_empty() || value.len() > max_bytes {
        return Err(Failure::usage(&format!("invalid {name}")));
    }
    *index += 1;
    Ok(value.clone())
}

fn take_sql_path(args: &[String], index: &mut usize) -> Result<String, Failure> {
    let Some(value) = args.get(*index + 1) else {
        return Err(Failure::usage("missing flag value"));
    };
    if value == "-" {
        *index += 1;
        return Ok(value.clone());
    }
    take_text(args, index, "sql file", PATH_MAX_BYTES)
}

fn take_number(
    args: &[String],
    index: &mut usize,
    name: &str,
    min: u32,
    max: u32,
) -> Result<u32, Failure> {
    let Some(value) = args.get(*index + 1) else {
        return Err(Failure::usage("missing flag value"));
    };
    if value.is_empty()
        || value.len() > 10
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(Failure::usage(&format!("invalid {name}")));
    }
    let Ok(number) = value.parse::<u32>() else {
        return Err(Failure::usage(&format!("invalid {name}")));
    };
    if !(min..=max).contains(&number) {
        return Err(Failure::usage(&format!("invalid {name}")));
    }
    *index += 1;
    Ok(number)
}

fn parse_transport(value: &str) -> Result<Transport, Failure> {
    match value {
        "production" => Ok(Transport::Production),
        "local-development" => Ok(Transport::LocalDevelopment),
        _ => Err(Failure::usage(
            "transport must be production or local-development",
        )),
    }
}

fn parse_auth(value: &str) -> Result<Auth, Failure> {
    match value {
        "external" => Ok(Auth::External),
        "sandbox" => Ok(Auth::Sandbox),
        _ => Err(Failure::usage("auth must be external or sandbox")),
    }
}

fn unknown_flag(arg: &str) -> Failure {
    if is_safe_flag(arg) {
        Failure::usage(&format!("unknown flag: {arg}"))
    } else {
        Failure::usage("unknown flag")
    }
}

fn is_safe_flag(arg: &str) -> bool {
    let Some(name) = arg.strip_prefix("--") else {
        return false;
    };
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn top_help() -> String {
    format!("{}\n\n{}", TOP, EXAMPLES)
}

pub fn query_help() -> String {
    format!("oo query — one read-only SQL query\n\n{QUERY_FLAGS}\n\n{EXAMPLES}\n\n{NOTES}")
}

fn unsupported_help(command: &str) -> String {
    let shown = if is_safe_command(command) {
        command
    } else {
        "that command"
    };
    format!("{shown} is not implemented in this slice\n\n{TOP}\n\n{EXAMPLES}")
}

fn is_safe_command(command: &str) -> bool {
    !command.is_empty()
        && command.len() <= 32
        && command
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

const TOP: &str = "\
oo — data CLI

  oo query [options] \"<sql>\"
  oo query [options] -f <file.sql>

Only query is implemented. Other commands are refused.";

const EXAMPLES: &str = "\
Examples:
  oo query --auth external --token-file ./token --gateway https://gateway.example --transport production \"SELECT 1\"
  oo query --json --auth external --token-stdin --gateway http://127.0.0.1:9 --transport local-development -f query.sql";

const QUERY_FLAGS: &str = "\
  --json                 typed JSON on stdout, one trailing newline
  --gateway <url>        origin only; the client appends /v1/query
  --transport <mode>     production or local-development
  --auth <mode>          external or sandbox
  --token-file <file>    external token, mode 0600, not a symlink
  --token-stdin          external token from stdin, not from the SQL
  --ca-file <file>       PEM roots for this process only
  --max-rows <n>         1..=2000
  --timeout-ms <n>       1..=120000; shortens the client wait, never raises it
  -f <file.sql>          exact UTF-8 SQL, at most 65536 bytes";

const NOTES: &str = "\
Sandbox mode stops before a token, OO_TOKEN, or home read. External mode reads
only the named token source. Exit 0 is success, including an explicit truncated
result. Exit 2 is usage or a non-retryable Gateway refusal. Exit 3 is a
retryable RATE_LIMITED or OVERLOADED result after the SDK admission policy.
Exit 4 is authentication or network. Exit 1 is everything else.
The wire format is JSON. Markdown is rendered locally from that response.";

pub fn unsupported_failure() -> Failure {
    Failure::unsupported()
}
