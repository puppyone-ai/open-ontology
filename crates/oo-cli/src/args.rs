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
    pub client_pem: Option<String>,
    pub max_rows: Option<u32>,
    pub timeout_ms: Option<u32>,
    pub sql: Option<String>,
    pub sql_file: Option<String>,
}

pub struct ConnArgs {
    pub json: bool,
    pub gateway: Option<String>,
    pub transport: Option<Transport>,
    pub auth: Option<Auth>,
    pub token_file: Option<String>,
    pub token_stdin: bool,
    pub ca_file: Option<String>,
    pub client_pem: Option<String>,
}

pub enum Invocation {
    Help(String),
    Unsupported,
    Query(QueryArgs),
    Tables(ConnArgs),
    Describe(ConnArgs, String),
    Validate(ConnArgs, String),
    Freshness(ConnArgs),
    KnowledgeCat(ConnArgs, String),
    KnowledgeSearch(ConnArgs, String),
    Cancel(ConnArgs, String),
    Status(ConnArgs, String),
    Operations(ConnArgs),
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
    match command.as_str() {
        "query" => parse_query(&args[1..]),
        "tables" => parse_tables(&args[1..]),
        "describe" => parse_describe(&args[1..]),
        "validate" => parse_validate(&args[1..]),
        "sql" => parse_sql(&args[1..]),
        "freshness" => parse_freshness(&args[1..]),
        "knowledge" => parse_knowledge(&args[1..]),
        "cancel" => parse_query_id(&args[1..], true),
        "status" => parse_query_id(&args[1..], false),
        "operations" => parse_operations(&args[1..]),
        _ => {
            if args.iter().any(|arg| arg == "--help" || arg == "-h") {
                return Ok(Invocation::Help(unsupported_help(command)));
            }
            Ok(Invocation::Unsupported)
        }
    }
}

fn parse_tables(args: &[String]) -> Result<Invocation, Failure> {
    if wants_help(args) {
        return Ok(Invocation::Help(command_help(
            "oo tables — tables in the token pack",
        )));
    }
    let parsed = parse_conn_flags(args)?;
    if !parsed.positional.is_empty() {
        return Err(Failure::usage("extra argument"));
    }
    Ok(Invocation::Tables(parsed.conn))
}

fn parse_describe(args: &[String]) -> Result<Invocation, Failure> {
    if wants_help(args) {
        return Ok(Invocation::Help(command_help(
            "oo describe — one schema.table in the token pack",
        )));
    }
    let parsed = parse_conn_flags(args)?;
    let table = one_positional(&parsed.positional, "table is missing")?;
    if !table_ok(&table) {
        return Err(Failure::usage("table is not public"));
    }
    Ok(Invocation::Describe(parsed.conn, table))
}

fn parse_validate(args: &[String]) -> Result<Invocation, Failure> {
    if wants_help(args) {
        return Ok(Invocation::Help(command_help(
            "oo validate — admit SQL without reading rows",
        )));
    }
    let parsed = parse_conn_flags(args)?;
    let sql = one_positional(&parsed.positional, "sql is missing")?;
    if sql.is_empty() {
        return Err(Failure::usage("sql is missing"));
    }
    Ok(Invocation::Validate(parsed.conn, sql))
}

fn parse_sql(args: &[String]) -> Result<Invocation, Failure> {
    if args.first().map(String::as_str) != Some("validate") {
        if wants_help(args) {
            return Ok(Invocation::Help(unsupported_help("sql")));
        }
        return Ok(Invocation::Unsupported);
    }
    parse_validate(&args[1..])
}

fn parse_freshness(args: &[String]) -> Result<Invocation, Failure> {
    if wants_help(args) {
        return Ok(Invocation::Help(command_help(
            "oo freshness — freshness for the token role",
        )));
    }
    let parsed = parse_conn_flags(args)?;
    if !parsed.positional.is_empty() {
        return Err(Failure::usage("extra argument"));
    }
    Ok(Invocation::Freshness(parsed.conn))
}

fn parse_knowledge(args: &[String]) -> Result<Invocation, Failure> {
    if args.is_empty() || wants_help(args) && args.iter().all(|arg| arg != "cat" && arg != "search")
    {
        if wants_help(args) {
            return Ok(Invocation::Help(command_help(
                "oo knowledge cat <path>\noo knowledge search <literal>",
            )));
        }
        return Err(Failure::usage("knowledge command is missing"));
    }
    match args[0].as_str() {
        "cat" => {
            if wants_help(&args[1..]) {
                return Ok(Invocation::Help(command_help("oo knowledge cat <path>")));
            }
            let parsed = parse_conn_flags(&args[1..])?;
            let path = one_positional(&parsed.positional, "path is missing")?;
            if !knowledge_path_ok(&path) {
                return Err(Failure::usage("path is not public"));
            }
            Ok(Invocation::KnowledgeCat(parsed.conn, path))
        }
        "search" => {
            if wants_help(&args[1..]) {
                return Ok(Invocation::Help(command_help(
                    "oo knowledge search <literal>",
                )));
            }
            let parsed = parse_conn_flags(&args[1..])?;
            let needle = one_positional(&parsed.positional, "search text is missing")?;
            if needle.is_empty() || needle.len() > 200 || needle.contains('\0') {
                return Err(Failure::usage("search text is not public"));
            }
            Ok(Invocation::KnowledgeSearch(parsed.conn, needle))
        }
        _ => {
            if wants_help(args) {
                return Ok(Invocation::Help(unsupported_help("knowledge")));
            }
            Ok(Invocation::Unsupported)
        }
    }
}

fn parse_operations(args: &[String]) -> Result<Invocation, Failure> {
    if wants_help(args) {
        return Ok(Invocation::Help(command_help(
            "oo operations — running queries for this token",
        )));
    }
    let parsed = parse_conn_flags(args)?;
    if !parsed.positional.is_empty() {
        return Err(Failure::usage("extra argument"));
    }
    Ok(Invocation::Operations(parsed.conn))
}

fn parse_query_id(args: &[String], cancel: bool) -> Result<Invocation, Failure> {
    if wants_help(args) {
        let text = if cancel {
            "oo cancel <query_id>"
        } else {
            "oo status <query_id>"
        };
        return Ok(Invocation::Help(command_help(text)));
    }
    let parsed = parse_conn_flags(args)?;
    let query_id = one_positional(&parsed.positional, "query id is missing")?;
    if !query_id_ok(&query_id) {
        return Err(Failure::usage("query id is not public"));
    }
    if cancel {
        Ok(Invocation::Cancel(parsed.conn, query_id))
    } else {
        Ok(Invocation::Status(parsed.conn, query_id))
    }
}

struct ConnParse {
    conn: ConnArgs,
    positional: Vec<String>,
}

fn parse_conn_flags(args: &[String]) -> Result<ConnParse, Failure> {
    let mut json = false;
    let mut gateway = None;
    let mut transport = None;
    let mut auth = None;
    let mut token_file = None;
    let mut token_stdin = false;
    let mut ca_file = None;
    let mut client_pem = None;
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
                "--client-pem" => {
                    let value = take_text(args, &mut index, "client-pem", PATH_MAX_BYTES)?;
                    once(&mut client_pem, value)?;
                }
                _ => return Err(unknown_flag(arg)),
            }
        } else {
            positional.push(args[index].clone());
        }
        index += 1;
    }
    if token_file.is_some() && token_stdin {
        return Err(Failure::usage(
            "token-file and token-stdin are mutually exclusive",
        ));
    }
    Ok(ConnParse {
        conn: ConnArgs {
            json,
            gateway,
            transport,
            auth,
            token_file,
            token_stdin,
            ca_file,
            client_pem,
        },
        positional,
    })
}

fn one_positional(positional: &[String], missing: &str) -> Result<String, Failure> {
    if positional.len() > 1 {
        return Err(Failure::usage("extra argument"));
    }
    match positional.first() {
        Some(text) if text.is_empty() => Err(Failure::usage(missing)),
        Some(text) => Ok(text.clone()),
        None => Err(Failure::usage(missing)),
    }
}

fn table_ok(table: &str) -> bool {
    oo_core::text::QualifiedTable::new(table).is_ok()
        && !table.contains('/')
        && !table.contains('\\')
}

fn knowledge_path_ok(path: &str) -> bool {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.len() > 512
    {
        return false;
    }
    path.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    })
}

fn query_id_ok(value: &str) -> bool {
    let rest = value
        .strip_prefix("q_")
        .or_else(|| value.strip_prefix("s_"));
    let Some(rest) = rest else {
        return false;
    };
    (6..=40).contains(&rest.len()) && rest.bytes().all(|byte| byte.is_ascii_alphanumeric())
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
    let mut client_pem = None;
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
                "--client-pem" => {
                    let value = take_text(args, &mut index, "client-pem", PATH_MAX_BYTES)?;
                    once(&mut client_pem, value)?;
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
        client_pem,
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
  oo tables [options]
  oo describe [options] <schema.table>
  oo validate [options] \"<sql>\"
  oo sql validate [options] \"<sql>\"
  oo freshness [options]
  oo knowledge cat [options] <path>
  oo knowledge search [options] <literal>
  oo cancel [options] <query_id>
  oo status [options] <query_id>
  oo operations [options]

export, config, and mcp are not implemented.";

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
  --client-pem <file>    client certificate and key, this process only
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

fn command_help(title: &str) -> String {
    format!("{title}\n\n{CONN_FLAGS}\n\n{EXAMPLES}\n\n{NOTES}")
}

const CONN_FLAGS: &str = "\
  --json                 JSON on stdout, one trailing newline
  --gateway <url>        origin only; the client appends the public path
  --transport <mode>     production or local-development
  --auth <mode>          external or sandbox
  --token-file <file>    external token, mode 0600, not a symlink
  --token-stdin          external token from stdin
  --ca-file <file>       PEM roots for this process only
  --client-pem <file>    client certificate and key, this process only";
