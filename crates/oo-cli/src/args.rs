//! Consumer flags are pure. Admin dispatch validates arguments before local IO.

#[path = "admin/mod.rs"]
mod admin;

use std::net::IpAddr;

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
    pub publication_binding: Option<oo_sdk::PublicationBinding>,
    pub json: bool,
    pub gateway: Option<String>,
    pub connect_ip: Option<IpAddr>,
    pub interface: Option<String>,
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
    pub publication_binding: Option<oo_sdk::PublicationBinding>,
    pub json: bool,
    pub gateway: Option<String>,
    pub connect_ip: Option<IpAddr>,
    pub interface: Option<String>,
    pub transport: Option<Transport>,
    pub auth: Option<Auth>,
    pub token_file: Option<String>,
    pub token_stdin: bool,
    pub ca_file: Option<String>,
    pub client_pem: Option<String>,
}

pub struct SearchArgs {
    pub needle: String,
    pub path_prefix: Option<String>,
    pub offset: u32,
    pub limit: u32,
}

pub enum Invocation {
    AdminReport(String, u8),
    Help(String),
    AgentSkill(bool),
    Unsupported,
    Query(QueryArgs),
    Tables(ConnArgs),
    Describe(ConnArgs, String, bool),
    Validate(ConnArgs, String),
    Freshness(ConnArgs, bool),
    KnowledgeCat(ConnArgs, String),
    KnowledgeSearch(ConnArgs, SearchArgs),
    Cancel(ConnArgs, String),
    Status(ConnArgs, String),
    Operations(ConnArgs),
}

pub fn parse() -> Result<Invocation, Failure> {
    let args = os_args()?;
    let args = expand_long_options(&args);
    if let Some(result) = crate::lifecycle::dispatch(&args) {
        return result;
    }
    if let Some(result) = admin::dispatch(&args) {
        return result;
    }
    let args = crate::lifecycle::prepare_data(args)?;
    parse_args(&args)
}

fn expand_long_options(args: &[String]) -> Vec<String> {
    let mut output = Vec::new();
    let mut positional = false;
    for arg in args {
        if arg == "--" {
            positional = true;
        }
        if !positional
            && let Some((flag, value)) = arg.split_once('=')
            && matches!(
                flag,
                "--gateway"
                    | "--transport"
                    | "--auth"
                    | "--token-file"
                    | "--ca-file"
                    | "--client-pem"
                    | "--connect-ip"
                    | "--interface"
                    | "--max-rows"
                    | "--timeout-ms"
                    | "--path-prefix"
                    | "--offset"
                    | "--limit"
                    | "--expected-publication-id"
                    | "--expected-data-vector-sha256"
                    | "--code"
                    | "--profile"
                    | "--business-timezone"
                    | "--grant-file"
                    | "--role"
                    | "--agent"
            )
        {
            output.extend([flag.to_owned(), value.to_owned()]);
        } else {
            output.push(arg.clone());
        }
    }
    output
}

#[cfg(test)]
mod long_option_tests {
    use super::expand_long_options;

    #[test]
    fn equals_options_never_rewrite_sql_values_or_post_separator_positionals() {
        let args = [
            "query",
            "--max-rows=10",
            "--gateway=https://gateway.example",
            "SELECT '--auth=external'",
            "--",
            "--token-file=literal.sql",
        ]
        .map(str::to_owned);
        assert_eq!(
            expand_long_options(&args),
            [
                "query",
                "--max-rows",
                "10",
                "--gateway",
                "https://gateway.example",
                "SELECT '--auth=external'",
                "--",
                "--token-file=literal.sql",
            ]
        );
        assert_eq!(
            expand_long_options(&["--json=true".into()]),
            ["--json=true"]
        );
    }
}

fn parse_args(args: &[String]) -> Result<Invocation, Failure> {
    if args.is_empty() {
        return Err(Failure::usage("command is missing"));
    }
    let command = &args[0];
    if command == "help" {
        return match &args[1..] {
            [] => Ok(Invocation::Help(top_help())),
            topics if topics.len() <= 2 && topics.iter().all(|topic| is_safe_command(topic)) => {
                let mut nested = topics.to_vec();
                if nested[0] == "help" {
                    return Err(Failure::usage("use oo help [command [subcommand]]"));
                }
                nested.push("--help".to_owned());
                parse_args(&nested)
            }
            _ => Err(Failure::usage("use oo help [command [subcommand]]")),
        };
    }
    if command == "--version" || command == "-V" {
        return match &args[1..] {
            [] => Ok(Invocation::Help(format!(
                "oo {}",
                env!("CARGO_PKG_VERSION")
            ))),
            [flag] if flag == "--json" => Ok(Invocation::Help(
                serde_json::json!({"name": "oo", "version": env!("CARGO_PKG_VERSION")}).to_string(),
            )),
            _ => Err(Failure::usage("use oo --version [--json]")),
        };
    }
    if command == "--help" || command == "-h" {
        return Ok(Invocation::Help(top_help()));
    }
    if command.starts_with('-') {
        return Err(Failure::usage("command is missing"));
    }
    match command.as_str() {
        "agent-skill" => {
            if wants_help(&args[1..]) {
                return Ok(Invocation::Help(
                    "oo agent-skill [--json] - offline business-agent guidance; no credentials are read".to_owned()));
            }
            let parsed = parse_conn_flags(&args[1..])?;
            if !parsed.positional.is_empty() {
                return Err(Failure::usage("extra argument"));
            }
            Ok(Invocation::AgentSkill(parsed.conn.json))
        }
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
        _ => Ok(Invocation::Unsupported),
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
            "Usage: oo describe [options] [--human] <schema.table>\n\nOn a terminal, list every visible column with its type and summarize query capabilities.\nPipes retain full JSON for existing scripts. Use --human for a readable table in a pipe,\nor --json for full metadata and exact function/operator signatures in any environment.\nUse oo validate \"<sql>\" to check the complete query against the current policy.",
        )));
    }
    let mut force_human = false;
    let mut ended = false;
    let mut rest = Vec::new();
    for arg in args {
        if arg == "--" {
            ended = true;
        }
        if !ended && arg == "--human" {
            set_flag(&mut force_human)?;
        } else {
            rest.push(arg.clone());
        }
    }
    let parsed = parse_conn_flags(&rest)?;
    if force_human && parsed.conn.json {
        return Err(Failure::usage(
            "describe --human and --json are mutually exclusive",
        ));
    }
    let table = one_positional(&parsed.positional, "table is missing")?;
    if !table_ok(&table) {
        return Err(Failure::usage("table is not public"));
    }
    Ok(Invocation::Describe(parsed.conn, table, force_human))
}

fn parse_validate(args: &[String]) -> Result<Invocation, Failure> {
    if wants_help(args) {
        return Ok(Invocation::Help(command_help(
            "Usage: oo validate [options] \"<sql>\"\n\nCheck SQL against the same Gateway policy used by query, without reading rows.\nValidation does not execute the query or freeze the current data.",
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
    if args.is_empty()
        || args
            .first()
            .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        return Ok(Invocation::Help(command_help(
            "Usage: oo sql validate [options] \"<sql>\"\n\nAlias for oo validate.",
        )));
    }
    if args.first().map(String::as_str) != Some("validate") {
        return Ok(Invocation::Unsupported);
    }
    parse_validate(&args[1..])
}

fn parse_freshness(args: &[String]) -> Result<Invocation, Failure> {
    if wants_help(args) {
        return Ok(Invocation::Help(command_help(
            "oo freshness [--summary] — freshness for the token role; summary omits evidence arrays, never warnings",
        )));
    }
    let parsed = parse_conn_flags_mode(args, true)?;
    if !parsed.positional.is_empty() {
        return Err(Failure::usage("extra argument"));
    }
    Ok(Invocation::Freshness(parsed.conn, parsed.summary))
}

#[cfg(test)]
mod freshness_tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn summary_is_explicit_and_only_valid_for_freshness() {
        assert!(matches!(
            parse_freshness(&strings(&["--json", "--summary"])),
            Ok(Invocation::Freshness(_, true))
        ));
        assert!(matches!(
            parse_freshness(&strings(&["--json"])),
            Ok(Invocation::Freshness(_, false))
        ));
        assert!(parse_tables(&strings(&["--summary"])).is_err());
        assert!(parse_freshness(&strings(&["--summary", "--summary"])).is_err());
        assert!(parse_freshness(&strings(&["--", "--summary"])).is_err());
    }
}

fn parse_knowledge(args: &[String]) -> Result<Invocation, Failure> {
    if args.is_empty()
        || args
            .first()
            .is_some_and(|arg| arg == "--help" || arg == "-h")
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
                    "oo knowledge search <literal> [--path-prefix tables/] [--offset 0] [--limit 20]",
                )));
            }
            let (remaining, path_prefix, offset, limit) = parse_search_options(&args[1..])?;
            let parsed = parse_conn_flags(&remaining)?;
            let needle = one_positional(&parsed.positional, "search text is missing")?;
            if needle.is_empty() || needle.len() > 200 || needle.contains('\0') {
                return Err(Failure::usage("search text is not public"));
            }
            Ok(Invocation::KnowledgeSearch(
                parsed.conn,
                SearchArgs {
                    needle,
                    path_prefix,
                    offset,
                    limit,
                },
            ))
        }
        _ => Ok(Invocation::Unsupported),
    }
}

fn parse_search_options(
    args: &[String],
) -> Result<(Vec<String>, Option<String>, u32, u32), Failure> {
    let mut remaining = Vec::new();
    let mut prefix = None;
    let mut offset = None;
    let mut limit = None;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        if flag == "--" {
            remaining.extend_from_slice(&args[index..]);
            break;
        }
        if matches!(flag, "--path-prefix" | "--offset" | "--limit") {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| Failure::usage("search option value is missing"))?;
            match flag {
                "--path-prefix" => {
                    if prefix.is_some()
                        || !knowledge_path_ok(value.strip_suffix('/').unwrap_or(value))
                    {
                        return Err(Failure::usage("invalid or repeated search path prefix"));
                    }
                    prefix = Some(value.clone());
                }
                "--offset" => {
                    let number: u32 = value
                        .parse()
                        .map_err(|_| Failure::usage("invalid search offset"))?;
                    if offset.replace(number).is_some() || number > 1_048_576 {
                        return Err(Failure::usage("invalid or repeated search offset"));
                    }
                }
                _ => {
                    let number: u32 = value
                        .parse()
                        .map_err(|_| Failure::usage("invalid search limit"))?;
                    if limit.replace(number).is_some() || !(1..=100).contains(&number) {
                        return Err(Failure::usage("invalid or repeated search limit"));
                    }
                }
            }
        } else {
            remaining.push(args[index].clone());
        }
        index += 1;
    }
    Ok((remaining, prefix, offset.unwrap_or(0), limit.unwrap_or(20)))
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
    summary: bool,
}

fn parse_conn_flags(args: &[String]) -> Result<ConnParse, Failure> {
    parse_conn_flags_mode(args, false)
}

fn parse_conn_flags_mode(args: &[String], allow_summary: bool) -> Result<ConnParse, Failure> {
    let mut summary = false;
    let mut json = false;
    let mut gateway = None;
    let mut connect_ip = None;
    let mut interface = None;
    let mut transport = None;
    let mut auth = None;
    let mut token_file = None;
    let mut token_stdin = false;
    let mut ca_file = None;
    let mut client_pem = None;
    let mut expected_publication = None;
    let mut expected_vector = None;
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
                return Err(Failure::usage(
                    "this flag does not accept an equals value; see oo help <command>",
                ));
            }
            match arg {
                "--summary" if allow_summary => set_flag(&mut summary)?,
                "--json" => set_flag(&mut json)?,
                "--token-stdin" => set_flag(&mut token_stdin)?,
                "--gateway" => {
                    let value = take_text(args, &mut index, "gateway", GATEWAY_MAX_BYTES)?;
                    once(&mut gateway, value)?;
                }
                "--connect-ip" => once(&mut connect_ip, take_ip(args, &mut index)?)?,
                "--interface" => {
                    let value = take_text(args, &mut index, "interface", 255)?;
                    once(&mut interface, value)?;
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
                "--expected-publication-id" => {
                    let value = take_text(args, &mut index, "expected-publication-id", 69)?;
                    once(&mut expected_publication, value)?;
                }
                "--expected-data-vector-sha256" => {
                    let value = take_text(args, &mut index, "expected-data-vector-sha256", 64)?;
                    once(&mut expected_vector, value)?;
                }
                "--client-pem" => {
                    let value = take_text(args, &mut index, "client-pem", PATH_MAX_BYTES)?;
                    once(&mut client_pem, value)?;
                }
                "--timeout-ms" | "--max-rows" | "-f" => {
                    return Err(Failure::usage(&format!(
                        "{arg} is query-only; use oo query --help"
                    )));
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
            publication_binding: parse_binding(expected_publication, expected_vector)?,
            json,
            gateway,
            connect_ip,
            interface,
            transport,
            auth,
            token_file,
            token_stdin,
            ca_file,
            client_pem,
        },
        positional,
        summary,
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
    let mut connect_ip = None;
    let mut interface = None;
    let mut transport = None;
    let mut auth = None;
    let mut token_file = None;
    let mut token_stdin = false;
    let mut ca_file = None;
    let mut client_pem = None;
    let mut expected_publication = None;
    let mut expected_vector = None;
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
                return Err(Failure::usage(
                    "this flag does not accept an equals value; see oo query --help",
                ));
            }
            match arg {
                "--json" => set_flag(&mut json)?,
                "--token-stdin" => set_flag(&mut token_stdin)?,
                "--gateway" => {
                    let value = take_text(args, &mut index, "gateway", GATEWAY_MAX_BYTES)?;
                    once(&mut gateway, value)?;
                }
                "--connect-ip" => once(&mut connect_ip, take_ip(args, &mut index)?)?,
                "--interface" => {
                    let value = take_text(args, &mut index, "interface", 255)?;
                    once(&mut interface, value)?;
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
                "--expected-publication-id" => {
                    let value = take_text(args, &mut index, "expected-publication-id", 69)?;
                    once(&mut expected_publication, value)?;
                }
                "--expected-data-vector-sha256" => {
                    let value = take_text(args, &mut index, "expected-data-vector-sha256", 64)?;
                    once(&mut expected_vector, value)?;
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
        publication_binding: parse_binding(expected_publication, expected_vector)?,
        json,
        gateway,
        connect_ip,
        interface,
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

fn parse_binding(
    id: Option<String>,
    vector: Option<String>,
) -> Result<Option<oo_sdk::PublicationBinding>, Failure> {
    match (id.as_deref(), vector.as_deref()) {
        (None, None) => Ok(None),
        (Some("current"), None) => Ok(Some(oo_sdk::PublicationBinding::current())),
        (Some(id), vector) => oo_sdk::PublicationBinding::expected(id, vector)
            .map(Some)
            .map_err(|_| Failure::usage("invalid publication binding")),
        _ => Err(Failure::usage(
            "data vector requires expected-publication-id",
        )),
    }
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
    let flag = if name == "sql file" {
        "-f".to_owned()
    } else {
        format!("--{name}")
    };
    let Some(value) = args.get(*index + 1) else {
        return Err(Failure::usage(&format!("missing flag value for {flag}")));
    };
    if value.starts_with('-') {
        return Err(Failure::usage(&format!("missing flag value for {flag}")));
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

fn take_ip(args: &[String], index: &mut usize) -> Result<IpAddr, Failure> {
    take_text(args, index, "connect-ip", 45)?
        .parse()
        .map_err(|_| Failure::usage("connect-ip must be an IPv4 or IPv6 address without a port"))
}

#[cfg(test)]
mod route_tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn query_and_public_flags_accept_only_explicit_literal_routes() {
        for ip in ["127.0.0.1", "::1", "2001:db8::1"] {
            let mut flags = strings(&["--connect-ip", ip, "--interface", "en0"]);
            let Ok(parsed) = parse_conn_flags(&flags) else {
                panic!("connection flags");
            };
            let conn = parsed.conn;
            assert_eq!(conn.connect_ip, Some(ip.parse().unwrap()));
            assert_eq!(conn.interface.as_deref(), Some("en0"));
            flags.push("select 1".to_owned());
            let Ok(Invocation::Query(query)) = parse_query(&flags) else {
                panic!("query flags");
            };
            assert_eq!(query.connect_ip, conn.connect_ip);
            assert_eq!(query.interface, conn.interface);
        }
        for flags in [
            vec!["--connect-ip"],
            vec!["--interface"],
            vec!["--connect-ip", "https://127.0.0.1"],
            vec!["--connect-ip", "127.0.0.1:443"],
            vec!["--connect-ip", "[::1]"],
            vec!["--connect-ip", "gateway.test"],
            vec!["--connect-ip", "::1", "--connect-ip", "::1"],
            vec!["--interface", "en0", "--interface", "en0"],
        ] {
            let flags = strings(&flags);
            assert!(parse_conn_flags(&flags).is_err());
            assert!(parse_query(&flags).is_err());
        }
        let sql = "SELECT '--connect-ip 192.0.2.1 --interface evil'";
        let Ok(Invocation::Query(query)) = parse_query(&strings(&["--", sql])) else {
            panic!("SQL remains data");
        };
        assert!(query.connect_ip.is_none() && query.interface.is_none());
        assert_eq!(query.sql.as_deref(), Some(sql));
        let Ok(parsed) = parse_conn_flags(&[]) else {
            panic!("default flags");
        };
        let conn = parsed.conn;
        assert!(conn.connect_ip.is_none() && conn.interface.is_none());
    }

    #[test]
    fn route_help_states_operator_boundary_and_failure_behavior() {
        for help in [top_help(), query_help(), command_help("tables")] {
            for required in [
                "--connect-ip",
                "--interface",
                "Operator-only",
                "TLS SNI",
                "hostname verification",
                "token/audience",
                "Proxies remain disabled",
                "Unsupported platforms fail",
                "without fallback",
            ] {
                assert!(help.contains(required), "{required}");
            }
        }
    }
}

fn take_number(
    args: &[String],
    index: &mut usize,
    name: &str,
    min: u32,
    max: u32,
) -> Result<u32, Failure> {
    let Some(value) = args.get(*index + 1) else {
        return Err(Failure::usage(&format!(
            "missing flag value for --{name}; use an integer in {min}..={max}"
        )));
    };
    if value.is_empty()
        || value.len() > 10
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(Failure::usage(&format!(
            "invalid {name}; use an integer in {min}..={max}"
        )));
    }
    let Ok(number) = value.parse::<u32>() else {
        return Err(Failure::usage(&format!(
            "invalid {name}; use an integer in {min}..={max}"
        )));
    };
    if !(min..=max).contains(&number) {
        return Err(Failure::usage(&format!(
            "invalid {name}; use an integer in {min}..={max}"
        )));
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
    format!(
        "{TOP}\n\n{}\n\n{}\n\n{QUERY_FLAGS}\n\n{SEARCH_FLAGS}\n\n{ROUTE_FLAGS}\n\n{BINDING_FLAGS}\n\n{EXAMPLES}\n\n{SQL_NOTES}\n\n{NOTES}",
        admin::HELP,
        crate::lifecycle::HELP
    )
}

pub fn query_help() -> String {
    format!(
        "Usage: oo query [options] \"<sql>\"\n       oo query [options] -f <file.sql>\n\nRun one read-only SQL query.\n\n{QUERY_FLAGS}\n\n{ROUTE_FLAGS}\n\n{BINDING_FLAGS}\n\n{EXAMPLES}\n\n{SQL_NOTES}\n\nQuery lifecycle:\n  One budget covers admission, metadata, session and SQL; the effective limit is\n  the minimum of your request and Gateway policy (at most 30s). The client reserves\n  1500ms within its 30s transport cap to receive an error receipt.\n  TIMEOUT is not retried and never means zero rows. No partial aggregate is returned.\n  oo operations lists your active query_id values; oo cancel <query_id> requests\n  bounded cleanup; oo status <query_id> reports running/succeeded/failed/timed_out/\n  canceled/abandoned. These records are bounded, process-local, not durable history.\n  A busy role returns OVERLOADED instead of silently blocking other roles.\n  Cancellation/status require the same principal, project and role.\n\n{NOTES}"
    )
}

fn is_safe_command(command: &str) -> bool {
    !command.is_empty()
        && command.len() <= 32
        && command
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

const TOP: &str = "\
oo - read-only Gateway data CLI

Usage: oo <command> [options] [arguments]
       oo help [command [subcommand]]
       oo --version [--json]

Commands:
  oo query [options] \"<sql>\"
  oo query [options] -f <file.sql>
  oo tables [options]
  oo describe [options] <schema.table>
  oo validate [options] \"<sql>\"
  oo sql validate [options] \"<sql>\"
  oo freshness [options] [--summary]
  oo knowledge cat [options] <path>
  oo knowledge search [options] <literal>
  oo cancel [options] <query_id>
  oo status [options] <query_id>
  oo operations [options]
  oo agent-skill [--json]        Optional offline business guidance

Use -h or --help on any command; help never reads credentials or connects.
Flags follow the command. Use -- before a positional argument beginning with '-'.
Long value options accept --name value or --name=value; boolean flags take no value.
The npm distribution also provides configure, config show and time; see its top-level help.
This native executable has no saved profile; supply explicit connection flags.";

const SQL_NOTES: &str = "\
SQL time filters (Gateway capability must still be validated):
  Match literals to the column type reported by describe:
  date:        DATE '2026-01-01'
  timestamp:   TIMESTAMP '2026-01-01 00:00:00'
  timestamptz: TIMESTAMPTZ '2026-01-01 00:00:00+00:00'
  Use >= lower AND < upper for a half-open range.
  Bare strings and DATE-to-timestamp comparisons are not implicit casts.
  Relative times, named zones and general casts are not supported.
  FUNCTION_NOT_ALLOWED can mean unsupported syntax, not denied column access.
Numeric and bigint query cells are exact JSON strings, not floating-point numbers.
No cast is needed to preserve SUM/COUNT precision. Unsupported SQL is refused,
not an empty result. Use describe for current capabilities and validate for SQL checks.";

const EXAMPLES: &str = "\
Examples:
  oo query --auth external --token-file ./token --gateway https://gateway.example --transport production \"SELECT 1\"
  oo query --json --auth external --token-stdin --gateway http://127.0.0.1:9 --transport local-development -f query.sql";

const QUERY_FLAGS: &str = "\
Connection/output options (apply to every online command):
  --json                 typed JSON on stdout, one trailing newline
  --gateway <url>        origin only; the client appends /v1/query
  --transport <mode>     production or local-development
  --auth <mode>          external or sandbox
  --token-file <file>    external token, mode 0600, not a symlink
  --token-stdin          external token from stdin, not from the SQL
  --ca-file <file>       PEM roots for this process only
  --client-pem <file>    client certificate and key, this process only
\nQuery-only options (not accepted by freshness or other metadata commands):
  --max-rows <n>         1..=2000
  --timeout-ms <n>       1..=120000; total server-work budget, capped by Gateway
  -f <file.sql>          exact UTF-8 SQL, at most 65536 bytes";

const SEARCH_FLAGS: &str = "\
Knowledge search options:
  --path-prefix <path>   restrict to a relative path prefix, e.g. tables/
  --offset <n>           0..=1048576, default 0
  --limit <n>            1..=100, default 20
Search is a literal substring, not a regular expression. Follow next_offset
while has_more is true; the first page is not the entire catalog.
Freshness option:
  --summary             omit evidence arrays, never warnings or source-time status";

const NOTES: &str = "\
Sandbox mode stops before a token, OO_TOKEN, or home read. External mode reads
only the named token source. Exit 0 is success, including an explicit truncated
result. Exit 2 is usage or a non-retryable Gateway refusal. Exit 3 is a
retryable RATE_LIMITED or OVERLOADED result after the SDK admission policy.
Exit 4 is authentication or network. Exit 1 is everything else.
The wire format is JSON. Markdown is rendered locally from that response.";

const ROUTE_FLAGS: &str = "\
Operator-only routing (never supplied by SQL/model input; no environment fallback):
  --connect-ip <IPv4/IPv6>  override only the gateway DNS hostname's address, not its port
  --interface <interface>  bind sockets to this interface (e.g. en0)
The URL Host, TLS SNI, hostname verification and token/audience stay unchanged.
Proxies remain disabled. Defaults use normal DNS and routing.
connect-ip requires a DNS hostname; plaintext overrides must stay loopback.
Interface binding supports Linux/Android/Fuchsia, Apple OSes and Solaris/illumos.
Unsupported platforms fail explicitly; a missing interface fails without fallback.";

pub fn unsupported_failure() -> Failure {
    Failure::unsupported()
}

fn command_help(title: &str) -> String {
    format!("{title}\n\n{CONN_FLAGS}\n\n{ROUTE_FLAGS}\n\n{BINDING_FLAGS}\n\n{EXAMPLES}\n\n{NOTES}")
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

#[cfg(test)]
mod search_tests {
    use super::*;
    #[test]
    fn search_pagination_is_explicit_bounded_and_keeps_connection_arguments() {
        let args = [
            "search",
            "orders",
            "--path-prefix",
            "tables/",
            "--offset",
            "20",
            "--limit",
            "30",
            "--json",
        ]
        .map(str::to_owned);
        let Ok(Invocation::KnowledgeSearch(conn, query)) = parse_knowledge(&args) else {
            panic!("search");
        };
        assert!(conn.json);
        assert_eq!(query.needle, "orders");
        assert_eq!(query.path_prefix.as_deref(), Some("tables/"));
        assert_eq!((query.offset, query.limit), (20, 30));
        for flags in [
            vec!["--limit", "0"],
            vec!["--limit", "101"],
            vec!["--offset", "-1"],
            vec!["--path-prefix", "../"],
            vec!["--limit", "2", "--limit", "3"],
        ] {
            let args = ["search", "orders"]
                .into_iter()
                .chain(flags)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            assert!(parse_knowledge(&args).is_err());
        }
    }
}

const BINDING_FLAGS: &str = "  --expected-publication-id current|pub1_<sha256>
  --expected-data-vector-sha256 <sha256>
Acquire current once, then reuse the returned binding for the whole question.
PUBLICATION_DRIFT requires discarding intermediate results and recomputing the whole question.";
