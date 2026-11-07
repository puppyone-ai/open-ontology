//! One query. The SDK performs admission retries. This process does not loop.

use oo_core::api::{QueryFormat, QueryRequest};
use oo_core::text::QuerySql;
use oo_sdk::{BearerToken, Client, ClientConfig, RootTrust};
use std::io::IsTerminal;

use crate::args::{
    Auth, ConnArgs, Invocation, QueryArgs, SearchArgs, Transport, parse, unsupported_failure,
};
use crate::load::{
    read_ca_file, read_client_pem, read_sql_file, read_token_file, read_token_stdin,
};
use crate::outcome::{
    self, Failure, emit, from_query_error, tightened_timeouts, write_stderr, write_stdout,
};
use crate::render::{self, RenderError};

pub fn run() -> u8 {
    let json = command_json_mode();
    match parse() {
        Ok(Invocation::AdminReport(text, exit)) => {
            let written = write_text(&text);
            if written == 0 { exit } else { written }
        }
        Err(failure) => emit(&failure, json),
        Ok(Invocation::Help(text)) => write_text(&text),
        Ok(Invocation::AgentSkill(json)) => {
            let text = include_str!("../../../tools/agent-contract/business-answer-policy.md");
            if json {
                write_text(
                    &serde_json::json!({
                        "protocol": "oo.agent-skill/v1", "name": "business-analysis",
                        "text": text, "offline": true, "grants_access": false,
                    })
                    .to_string(),
                )
            } else {
                write_text(text)
            }
        }
        Ok(Invocation::Unsupported) => emit(&unsupported_failure(), json),
        Ok(Invocation::Query(query)) => execute(query),
        Ok(Invocation::Tables(conn)) => execute_public(conn, PublicCall::Tables),
        Ok(Invocation::Describe(conn, table, human)) => {
            execute_public(conn, PublicCall::Describe(table, human))
        }
        Ok(Invocation::Validate(conn, sql)) => execute_public(conn, PublicCall::Validate(sql)),
        Ok(Invocation::Freshness(conn, summary)) => {
            execute_public(conn, PublicCall::Freshness(summary))
        }
        Ok(Invocation::KnowledgeCat(conn, path)) => {
            execute_public(conn, PublicCall::KnowledgeCat(path))
        }
        Ok(Invocation::KnowledgeSearch(conn, needle)) => {
            execute_public(conn, PublicCall::KnowledgeSearch(needle))
        }
        Ok(Invocation::Cancel(conn, query_id)) => {
            execute_public(conn, PublicCall::Cancel(query_id))
        }
        Ok(Invocation::Status(conn, query_id)) => {
            execute_public(conn, PublicCall::Status(query_id))
        }
        Ok(Invocation::Operations(conn)) => execute_public(conn, PublicCall::Operations),
    }
}

enum PublicCall {
    Tables,
    Describe(String, bool),
    Validate(String),
    Freshness(bool),
    KnowledgeCat(String),
    KnowledgeSearch(SearchArgs),
    Cancel(String),
    Status(String),
    Operations,
}

fn command_json_mode() -> bool {
    let mut ended = false;
    let mut args = std::env::args_os().skip(1);
    let Some(command) = args.next() else {
        return false;
    };
    let Some(command) = command.to_str() else {
        return false;
    };
    if command.starts_with('-') {
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
    let identity = match query.client_pem.as_deref() {
        Some(path) => match read_client_pem(path) {
            Ok(bytes) => Some(bytes),
            Err(failure) => return outcome::emit_guarded(&failure, json, &token, &sql, gateway),
        },
        None => None,
    };
    let request = match build_request(&query, &sql) {
        Ok(request) => request,
        Err(failure) => return outcome::emit_guarded(&failure, json, &token, &sql, gateway),
    };
    let config = match build_config(&query, ca, identity) {
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
    if let Err(failure) = crate::lifecycle::preflight(
        gateway,
        query.ca_file.as_deref(),
        query.connect_ip,
        query.interface.as_deref(),
        query.transport,
    ) {
        return outcome::emit_guarded(&failure, json, &token, &sql, gateway);
    }
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
    let client = match query.publication_binding {
        Some(binding) => client.with_publication_binding(binding),
        None => client,
    };
    let result = runtime.block_on(client.query_with_receipt(request));
    note_auth_cache(&client);
    match result {
        Ok(result) => deliver(json, &result.response, result.publication_binding.as_ref()),
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

fn build_config(
    query: &QueryArgs,
    ca: Option<Vec<u8>>,
    identity: Option<Vec<u8>>,
) -> Result<ClientConfig, Failure> {
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
    config = apply_route(config, query.connect_ip, query.interface.as_deref())?;
    apply_material(config, ca, identity)
}

fn execute_public(conn: ConnArgs, call: PublicCall) -> u8 {
    let json = conn.json;
    let ready = match connect(&conn) {
        Ok(ready) => ready,
        Err(code) => return code,
    };
    if !json {
        let _ = write_stderr(b"oo: calling gateway\n");
    }
    if let Err(failure) = crate::lifecycle::preflight(
        &ready.gateway,
        conn.ca_file.as_deref(),
        conn.connect_ip,
        conn.interface.as_deref(),
        conn.transport,
    ) {
        return outcome::emit_guarded(&failure, json, &ready.token, "", &ready.gateway);
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return outcome::emit_guarded(
                &Failure::local("runtime rejected"),
                json,
                &ready.token,
                "",
                &ready.gateway,
            );
        }
    };
    let result = runtime.block_on(async {
        match &call {
            PublicCall::Tables => ready.client.tables().await,
            PublicCall::Describe(table, _) => ready.client.describe(table).await,
            PublicCall::Validate(sql) => ready.client.sql_validate(sql).await,
            PublicCall::Freshness(_) => ready.client.freshness().await,
            PublicCall::KnowledgeCat(path) => ready.client.knowledge(path).await,
            PublicCall::KnowledgeSearch(query) => {
                ready
                    .client
                    .knowledge_search_page(
                        &query.needle,
                        query.path_prefix.as_deref(),
                        query.offset,
                        query.limit,
                    )
                    .await
            }
            PublicCall::Cancel(query_id) => ready.client.cancel(query_id).await,
            PublicCall::Status(query_id) => ready.client.query_status(query_id).await,
            PublicCall::Operations => ready.client.operations().await,
        }
    });
    note_auth_cache(&ready.client);
    match result {
        Ok(value) => deliver_public(json, &call, &value),
        Err(error) => outcome::emit_guarded(
            &from_query_error(&error, &ready.token, "", &ready.gateway),
            json,
            &ready.token,
            "",
            &ready.gateway,
        ),
    }
}

struct Ready {
    client: Client,
    token: String,
    gateway: String,
}

fn connect(conn: &ConnArgs) -> Result<Ready, u8> {
    let json = conn.json;
    if conn.auth == Some(Auth::Sandbox) {
        return Err(emit(&Failure::sandbox(), json));
    }
    if conn.auth != Some(Auth::External) {
        return Err(emit(
            &Failure::auth("authentication source is missing"),
            json,
        ));
    }
    if conn.token_file.is_none() && !conn.token_stdin {
        return Err(emit(&Failure::auth("token source is missing"), json));
    }
    let Some(gateway) = conn.gateway.clone() else {
        return Err(emit(&Failure::usage("gateway is required"), json));
    };
    if conn.transport.is_none() {
        return Err(emit(&Failure::usage("transport is required"), json));
    }
    let token = match conn.token_file.as_deref() {
        Some(path) => match read_token_file(path) {
            Ok(token) => token,
            Err(failure) => return Err(emit(&failure, json)),
        },
        None => match read_token_stdin() {
            Ok(token) => token,
            Err(failure) => return Err(emit(&failure, json)),
        },
    };
    let ca = match conn.ca_file.as_deref() {
        Some(path) => match read_ca_file(path) {
            Ok(bytes) => Some(bytes),
            Err(failure) => {
                return Err(outcome::emit_guarded(&failure, json, &token, "", &gateway));
            }
        },
        None => None,
    };
    let identity = match conn.client_pem.as_deref() {
        Some(path) => match read_client_pem(path) {
            Ok(bytes) => Some(bytes),
            Err(failure) => {
                return Err(outcome::emit_guarded(&failure, json, &token, "", &gateway));
            }
        },
        None => None,
    };
    let config = match build_public_config(conn, ca, identity) {
        Ok(config) => config,
        Err(failure) => return Err(outcome::emit_guarded(&failure, json, &token, "", &gateway)),
    };
    let bearer = match BearerToken::new(&token) {
        Ok(bearer) => bearer,
        Err(error) => {
            return Err(outcome::emit_guarded(
                &from_query_error(&error, &token, "", &gateway),
                json,
                &token,
                "",
                &gateway,
            ));
        }
    };
    match Client::new(&gateway, bearer, config) {
        Ok(client) => Ok(Ready {
            client: match conn.publication_binding.clone() {
                Some(binding) => client.with_publication_binding(binding),
                None => client,
            },
            token,
            gateway,
        }),
        Err(error) => Err(outcome::emit_guarded(
            &from_query_error(&error, &token, "", &gateway),
            json,
            &token,
            "",
            &gateway,
        )),
    }
}

fn build_public_config(
    conn: &ConnArgs,
    ca: Option<Vec<u8>>,
    identity: Option<Vec<u8>>,
) -> Result<ClientConfig, Failure> {
    let config = match conn.transport {
        Some(Transport::Production) => ClientConfig::production(),
        Some(Transport::LocalDevelopment) => ClientConfig::local_development(),
        None => return Err(Failure::usage("transport is required")),
    };
    let config = apply_route(config, conn.connect_ip, conn.interface.as_deref())?;
    apply_material(config, ca, identity)
}

fn apply_route(
    mut config: ClientConfig,
    connect_ip: Option<std::net::IpAddr>,
    interface: Option<&str>,
) -> Result<ClientConfig, Failure> {
    if let Some(ip) = connect_ip {
        config = config.with_connect_ip(ip);
    }
    if let Some(interface) = interface {
        config = config
            .with_interface(interface.to_owned())
            .map_err(|error| from_query_error(&error, "", "", ""))?;
    }
    Ok(config)
}

fn apply_material(
    mut config: ClientConfig,
    ca: Option<Vec<u8>>,
    identity: Option<Vec<u8>>,
) -> Result<ClientConfig, Failure> {
    if let Some(pem) = ca {
        config = config
            .with_trust(RootTrust::OnlyPem(pem))
            .map_err(|_| Failure::local("trust material rejected"))?;
    }
    if let Some(pem) = identity {
        config = config
            .with_identity(pem)
            .map_err(|_| Failure::local("client pem rejected: other"))?;
    }
    Ok(config)
}

fn note_auth_cache(client: &Client) {
    let Some(ms) = client.auth_cache_remaining_ms() else {
        return;
    };
    let line = format!("auth_cache_remaining_ms={ms}\n");
    let _ = write_stderr(line.as_bytes());
}

fn deliver_public(json: bool, call: &PublicCall, value: &serde_json::Value) -> u8 {
    let summary;
    let value = if matches!(call, PublicCall::Freshness(true)) {
        summary = render::freshness_summary(value);
        &summary
    } else {
        value
    };
    let bytes = if json {
        match serde_json::to_vec(value) {
            Ok(mut bytes) => {
                bytes.push(b'\n');
                bytes
            }
            Err(_) => return emit(&Failure::local("output exceeds the render budget"), json),
        }
    } else if matches!(call, PublicCall::Describe(_, true))
        || matches!(call, PublicCall::Describe(_, false)) && std::io::stdout().is_terminal()
    {
        match render::describe_markdown(value) {
            Ok(bytes) => bytes,
            Err(_) => {
                return emit(
                    &Failure::local(
                        "table description could not be rendered; use --json for the full response",
                    ),
                    false,
                );
            }
        }
    } else if let PublicCall::KnowledgeCat(_) = call {
        match value.get("text").and_then(serde_json::Value::as_str) {
            Some(text) => text.as_bytes().to_vec(),
            None => return emit(&Failure::local("knowledge text is missing"), false),
        }
    } else {
        match serde_json::to_vec(value) {
            Ok(mut bytes) => {
                bytes.push(b'\n');
                bytes
            }
            Err(_) => return emit(&Failure::local("output exceeds the render budget"), false),
        }
    };
    if write_stdout(&bytes).is_err() {
        let _ = write_stderr(b"error: output delivery failed\n");
        1
    } else {
        0
    }
}

fn deliver(
    json: bool,
    response: &oo_core::api::QueryResponse,
    binding: Option<&oo_sdk::PublicationReceipt>,
) -> u8 {
    let rendered = if json {
        render::response_json(response)
    } else {
        render::response_markdown(response)
    };
    let rendered = rendered.and_then(|bytes| render::with_publication(bytes, json, binding));
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
