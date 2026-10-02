//! One query. The SDK performs admission retries. This process does not loop.

use oo_core::api::{QueryFormat, QueryRequest};
use oo_core::text::QuerySql;
use oo_sdk::{BearerToken, Client, ClientConfig, RootTrust};

use crate::args::{Auth, Invocation, QueryArgs, Transport, parse, unsupported_failure};
use crate::load::{read_ca_file, read_sql_file, read_token_file, read_token_stdin};
use crate::outcome::{
    self, Failure, emit, from_query_error, tightened_timeouts, write_stderr, write_stdout,
};
use crate::render::{self, RenderError};

pub fn run() -> u8 {
    let json = query_json_mode();
    match parse() {
        Err(failure) => emit(&failure, json),
        Ok(Invocation::Help(text)) => write_text(&text),
        Ok(Invocation::Unsupported) => emit(&unsupported_failure(), false),
        Ok(Invocation::Query(query)) => execute(query),
    }
}

fn query_json_mode() -> bool {
    let mut ended = false;
    let mut args = std::env::args_os().skip(1);
    let Some(command) = args.next() else {
        return false;
    };
    if command.to_str() != Some("query") {
        return false;
    }
    for arg in args {
        let Some(text) = arg.to_str() else {
            return false;
        };
        if !ended && text == "--" {
            ended = true;
            continue;
        }
        if !ended && text == "--json" {
            return true;
        }
    }
    false
}

fn execute(query: QueryArgs) -> u8 {
    let json = query.json;
    if query.auth == Some(Auth::Sandbox) {
        return emit(&Failure::sandbox(), json);
    }
    if query.auth != Some(Auth::External) {
        return emit(&Failure::auth("authentication source is missing"), json);
    }
    if query.token_file.is_none() && !query.token_stdin {
        return emit(&Failure::auth("token source is missing"), json);
    }
    let Some(gateway) = query.gateway.as_deref() else {
        return emit(&Failure::usage("gateway is required"), json);
    };
    if query.transport.is_none() {
        return emit(&Failure::usage("transport is required"), json);
    }
    let token = match query.token_file.as_deref() {
        Some(path) => match read_token_file(path) {
            Ok(token) => token,
            Err(failure) => return emit(&failure, json),
        },
        None => match read_token_stdin() {
            Ok(token) => token,
            Err(failure) => return emit(&failure, json),
        },
    };
    let sql = match load_sql(&query) {
        Ok(sql) => sql,
        Err(failure) => return outcome::emit_guarded(&failure, json, &token, "", gateway),
    };
    let ca = match query.ca_file.as_deref() {
        Some(path) => match read_ca_file(path) {
            Ok(bytes) => Some(bytes),
            Err(failure) => return outcome::emit_guarded(&failure, json, &token, &sql, gateway),
        },
        None => None,
    };
    let request = match build_request(&query, &sql) {
        Ok(request) => request,
        Err(failure) => return outcome::emit_guarded(&failure, json, &token, &sql, gateway),
    };
    let config = match build_config(&query, ca) {
        Ok(config) => config,
        Err(failure) => return outcome::emit_guarded(&failure, json, &token, &sql, gateway),
    };
    let bearer = match BearerToken::new(&token) {
        Ok(bearer) => bearer,
        Err(error) => {
            return outcome::emit_guarded(
                &from_query_error(&error, &token, &sql, gateway),
                json,
                &token,
                &sql,
                gateway,
            );
        }
    };
    let client = match Client::new(gateway, bearer, config) {
        Ok(client) => client,
        Err(error) => {
            return outcome::emit_guarded(
                &from_query_error(&error, &token, &sql, gateway),
                json,
                &token,
                &sql,
                gateway,
            );
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return outcome::emit_guarded(
                &Failure::local("runtime rejected"),
                json,
                &token,
                &sql,
                gateway,
            );
        }
    };
    match runtime.block_on(client.query(request)) {
        Ok(response) => deliver(json, &response),
        Err(error) => outcome::emit_guarded(
            &from_query_error(&error, &token, &sql, gateway),
            json,
            &token,
            &sql,
            gateway,
        ),
    }
}

fn load_sql(query: &QueryArgs) -> Result<String, Failure> {
    if let Some(sql) = &query.sql {
        return Ok(sql.clone());
    }
    let Some(path) = query.sql_file.as_deref() else {
        return Err(Failure::usage("sql is missing"));
    };
    read_sql_file(path)
}

fn build_request(query: &QueryArgs, sql: &str) -> Result<QueryRequest, Failure> {
    let sql = QuerySql::new(sql).map_err(|_| Failure::local("query rejected"))?;
    Ok(QueryRequest {
        sql,
        max_rows: query.max_rows,
        timeout_ms: query.timeout_ms,
        format: QueryFormat::Json,
        client_context: None,
    })
}

fn build_config(query: &QueryArgs, ca: Option<Vec<u8>>) -> Result<ClientConfig, Failure> {
    let mut config = match query.transport {
        Some(Transport::Production) => ClientConfig::production(),
        Some(Transport::LocalDevelopment) => ClientConfig::local_development(),
        None => return Err(Failure::usage("transport is required")),
    };
    if let Some(timeout_ms) = query.timeout_ms {
        config = config
            .with_timeouts(tightened_timeouts(timeout_ms))
            .map_err(|error| from_query_error(&error, "", "", ""))?;
    }
    if let Some(pem) = ca {
        config = config
            .with_trust(RootTrust::OnlyPem(pem))
            .map_err(|_| Failure::local("trust material rejected"))?;
    }
    Ok(config)
}

fn deliver(json: bool, response: &oo_core::api::QueryResponse) -> u8 {
    let rendered = if json {
        render::response_json(response)
    } else {
        render::response_markdown(response)
    };
    match rendered {
        Ok(bytes) => {
            if write_stdout(&bytes).is_err() {
                let _ = write_stderr(b"error: output delivery failed\n");
                1
            } else {
                0
            }
        }
        Err(RenderError::Budget | RenderError::Encode) => {
            emit(&Failure::local("output exceeds the render budget"), json)
        }
    }
}

fn write_text(text: &str) -> u8 {
    let mut bytes = text.as_bytes().to_vec();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    if write_stdout(&bytes).is_err() {
        let _ = write_stderr(b"error: output delivery failed\n");
        1
    } else {
        0
    }
}
