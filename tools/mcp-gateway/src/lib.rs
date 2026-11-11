//! Stdio MCP server for the existing loopback Gateway.
//!
//! Protocol handling is `rmcp` 3.5.0. This crate supplies the Gateway calls,
//! the strict version gate, and the refusal of remote export.

#![forbid(unsafe_code)]

mod http;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use http::{GatewayClient, parse_loopback_port};
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, DiscoverResult, ErrorCode,
    Implementation, InitializeRequestParams, InitializeResult, ListPromptsResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
    ResourceContents, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt};
use serde_json::{Map as JsonMap, Value};

const SUPPORTED: &[ProtocolVersion] =
    &[ProtocolVersion::V_2025_11_25, ProtocolVersion::V_2026_07_28];

const READY_LINE: &str = "oo-mcp-gateway: stdio ready oauth=false remote_export=disabled";

struct GatewayServer {
    client: GatewayClient,
}

pub fn run() -> i32 {
    let server = match GatewayServer::from_env() {
        Ok(server) => server,
        Err(reason) => {
            eprintln!("oo-mcp-gateway: method=startup status=1 reason={reason}");
            return 1;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("oo-mcp-gateway: method=startup status=1 reason=runtime");
            return 1;
        }
    };
    runtime.block_on(server.serve_stdio())
}

impl GatewayServer {
    fn from_env() -> Result<Self, &'static str> {
        if let Ok(name) = std::env::var("OO_MCP_PROFILE") {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return Err("profile");
            }
            let cli = PathBuf::from(std::env::var_os("OO_MCP_CLI").ok_or("cli")?);
            if !cli.is_absolute() || !cli.is_file() {
                return Err("cli");
            }
            return Ok(Self {
                client: GatewayClient::profile(cli, name),
            });
        }
        let url = std::env::var("OO_MCP_GATEWAY_URL").map_err(|_| "gateway_url")?;
        let port = parse_loopback_port(&url)?;
        let token_file = std::env::var("OO_MCP_TOKEN_FILE").map_err(|_| "token_file")?;
        if token_file.is_empty() {
            return Err("token_file");
        }
        Ok(Self {
            client: GatewayClient::new(port, PathBuf::from(token_file)),
        })
    }

    async fn serve_stdio(self) -> i32 {
        eprintln!("{READY_LINE}");
        let running = match self.serve(rmcp::transport::stdio()).await {
            Ok(running) => running,
            Err(_) => {
                eprintln!("oo-mcp-gateway: method=serve status=1");
                return 1;
            }
        };
        match running.waiting().await {
            Ok(_) => 0,
            Err(_) => {
                eprintln!("oo-mcp-gateway: method=serve status=1");
                1
            }
        }
    }

    async fn gateway_tool(
        &self,
        path: &str,
        body: Option<Value>,
    ) -> Result<CallToolResult, McpError> {
        let reply = match body {
            None => self.client.get(path).await,
            Some(body) => self.client.post_json(path, &body).await,
        };
        match reply {
            Ok(reply) => {
                log_status("tools/call", reply.status);
                if (200..300).contains(&reply.status) && reply.body.get("error").is_none() {
                    Ok(CallToolResult::structured(reply.body))
                } else {
                    Ok(CallToolResult::structured_error(reply.body))
                }
            }
            Err(()) => {
                eprintln!("oo-mcp-gateway: method=tools/call status=transport");
                Err(McpError::internal_error("gateway request failed", None))
            }
        }
    }
}

impl ServerHandler for GatewayServer {
    fn get_info(&self) -> ServerConfig {
        server_config()
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED)
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        if request.protocol_version != ProtocolVersion::V_2025_11_25 {
            log_status("initialize", 32022);
            return Err(McpError::unsupported_protocol_version(
                request.protocol_version,
                &[ProtocolVersion::V_2025_11_25],
            ));
        }
        context.peer.set_peer_info(request);
        log_status("initialize", 200);
        Ok(server_config())
    }

    async fn discover(
        &self,
        _context: RequestContext<RoleServer>,
    ) -> Result<DiscoverResult, McpError> {
        log_status("server/discover", 200);
        Ok(DiscoverResult::from_server_info(
            self.supported_protocol_versions().into_owned(),
            server_config(),
        ))
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        reject_cursor("tools/list", &request)?;
        log_status("tools/list", 200);
        Ok(ListToolsResult::with_all_items(tool_list())
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        match request.name.as_ref() {
            "session_current" => {
                reject_arguments(&request)?;
                self.gateway_tool("/v1/session/current", None)
                    .await
                    .map(CallToolResponse::from)
            }
            "pack_metadata" => {
                reject_arguments(&request)?;
                self.gateway_tool("/v1/pack/metadata", None)
                    .await
                    .map(CallToolResponse::from)
            }
            "query" => {
                let sql = required_sql(request.arguments.as_ref())?;
                let path = if self.client.is_profile() {
                    "/v1/query"
                } else {
                    "/v1/sql"
                };
                self.gateway_tool(path, Some(query_body(&sql, self.client.is_profile())))
                    .await
                    .map(CallToolResponse::from)
            }
            name if TOOL_ROUTES.iter().any(|(n, _, _)| *n == name) => {
                let (path, body) = tool_request(name, request.arguments.as_ref())?;
                self.gateway_tool(&path, body)
                    .await
                    .map(CallToolResponse::from)
            }
            _ => {
                log_status("tools/call", 32601);
                Err(McpError::new(
                    ErrorCode::METHOD_NOT_FOUND,
                    "unknown tool",
                    None,
                ))
            }
        }
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        reject_cursor("resources/list", &request)?;
        let reply = match self.client.get("/v1/pack/metadata").await {
            Ok(reply) => reply,
            Err(()) => {
                eprintln!("oo-mcp-gateway: method=resources/list status=transport");
                return Err(McpError::internal_error("gateway request failed", None));
            }
        };
        if !(200..300).contains(&reply.status) {
            log_status("resources/list", reply.status);
            return Err(McpError::internal_error("gateway request failed", None));
        }
        let resources = resources_from_pack(&reply.body).map_err(|_| {
            log_status("resources/list", 32602);
            McpError::invalid_params("pack file path is not a knowledge resource", None)
        })?;
        log_status("resources/list", 200);
        Ok(ListResourcesResult::with_all_items(resources)
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let path = knowledge_path(&request.uri).map_err(|_| {
            log_status("resources/read", 32602);
            McpError::invalid_params("resource uri is not a pack path", None)
        })?;
        let reply = match self.client.get("/v1/pack/metadata").await {
            Ok(reply) => reply,
            Err(()) => {
                eprintln!("oo-mcp-gateway: method=resources/read status=transport");
                return Err(McpError::internal_error("gateway request failed", None));
            }
        };
        if !(200..300).contains(&reply.status) {
            log_status("resources/read", reply.status);
            return Err(McpError::internal_error("gateway request failed", None));
        }
        let file = file_object(&reply.body, path).ok_or_else(|| {
            log_status("resources/read", 32602);
            McpError::invalid_params("resource is not in the pack", None)
        })?;
        let text = serde_json::to_string(file).map_err(|_| {
            log_status("resources/read", 500);
            McpError::internal_error("gateway request failed", None)
        })?;
        log_status("resources/read", 200);
        let contents = ResourceContents::text(text, request.uri).with_mime_type("application/json");
        Ok(ReadResourceResponse::from(
            ReadResourceResult::new(vec![contents])
                .with_ttl_ms(0)
                .with_cache_scope(CacheScope::Private),
        ))
    }

    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        reject_cursor("prompts/list", &request)?;
        log_status("prompts/list", 200);
        Ok(ListPromptsResult::with_all_items(Vec::new())
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private))
    }
}

fn query_body(sql: &str, profile: bool) -> Value {
    let mut body = serde_json::json!({"sql": sql});
    if profile {
        body["format"] = Value::from("json");
    }
    body
}

fn server_config() -> ServerConfig {
    ServerConfig::new(
        ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .enable_prompts()
            .build(),
    )
    .with_protocol_version(ProtocolVersion::V_2025_11_25)
    .with_server_info(Implementation::new("oo-mcp-gateway", "0.1.0"))
    .with_instructions(
        "First check_readiness, then check_answerable (advisory, never blocks queries). Discover only the role's published semantic tables with list_tables, describe_table and read_knowledge; resolve entities and code values before validate_sql and query. Cite query_id and freshness. Report gaps and refusals honestly; no raw-database browsing or export. OAuth is not implemented.",
    )
}

fn tool_list() -> Vec<Tool> {
    let annotations = ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false);
    let mut tools = vec![
        Tool::new(
            "session_current",
            "GET /v1/session/current. The role comes from the bearer token.",
            empty_schema(),
        )
        .with_annotations(annotations.clone()),
        Tool::new(
            "pack_metadata",
            "GET /v1/pack/metadata. Returns the token's compiled pack descriptor.",
            empty_schema(),
        )
        .with_annotations(annotations.clone()),
        Tool::new(
            "query",
            "Execute read-only SQL through the role-scoped Gateway. Validate first; cite query_id and freshness. A refusal is not an empty result.",
            query_schema(),
        )
        .with_annotations(annotations),
    ];
    for (name, _, field) in TOOL_ROUTES {
        let schema = match field {
            Some(field) => {
                let mut properties = JsonMap::new();
                properties.insert(
                    (*field).into(),
                    serde_json::json!({"type":"string", "minLength":1, "maxLength":2048}),
                );
                if *name == "resolve_entity" {
                    properties.insert("entity".into(), serde_json::json!({"type":"string"}));
                }
                if *name == "search_values" {
                    properties.insert("column".into(), serde_json::json!({"type":"string"}));
                }
                let mut required = vec![*field];
                if *name == "resolve_entity" {
                    required.push("entity");
                }
                if *name == "search_values" {
                    required.push("column");
                }
                object_schema(properties, &required)
            }
            None => empty_schema(),
        };
        tools.push(Tool::new(*name, format!("Published role-scoped Gateway operation: {name}. A refusal is not an empty result."), schema)
            .with_annotations(ToolAnnotations::new().read_only(*name != "report_gap")
                .destructive(false).idempotent(*name != "report_gap").open_world(false)));
    }
    tools
}

const TOOL_ROUTES: &[(&str, &str, Option<&str>)] = &[
    ("list_tables", "/v1/tables", None),
    ("describe_table", "/v1/tables/", Some("table")),
    ("resolve_entity", "/v1/entities/", Some("text")),
    ("search_values", "/v1/values/search", Some("text")),
    ("validate_sql", "/v1/sql/validate", Some("sql")),
    ("get_freshness", "/v1/freshness", None),
    ("read_knowledge", "/v1/knowledge/", Some("path")),
    ("search_knowledge", "/v1/knowledge/search", Some("q")),
    ("report_gap", "/v1/feedback/gap", Some("description")),
    ("check_readiness", "/v1/check/readiness", None),
    ("check_answerable", "/v1/check/answerable", Some("question")),
];

fn tool_request(
    name: &str,
    args: Option<&JsonMap<String, Value>>,
) -> Result<(String, Option<Value>), McpError> {
    let (_, route, field) = TOOL_ROUTES
        .iter()
        .find(|(n, _, _)| *n == name)
        .ok_or_else(|| McpError::invalid_params("unknown tool", None))?;
    let empty = JsonMap::new();
    let args = args.unwrap_or(&empty);
    let bad = || McpError::invalid_params("invalid tool arguments", None);
    let Some(field) = field else {
        return if args.is_empty() {
            Ok(((*route).into(), None))
        } else {
            Err(bad())
        };
    };
    let extra = match name {
        "resolve_entity" => Some("entity"),
        "search_values" => Some("column"),
        _ => None,
    };
    if args.len() != 1 + usize::from(extra.is_some()) {
        return Err(bad());
    }
    let value = args
        .get(*field)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= 2048 && !s.contains('\0'))
        .ok_or_else(bad)?;
    if let Some(extra) = extra {
        let identifier = args
            .get(extra)
            .and_then(Value::as_str)
            .filter(|s| safe_identifier(s))
            .ok_or_else(bad)?;
        if name == "resolve_entity" {
            return Ok((
                format!("{route}{identifier}/resolve"),
                Some(serde_json::json!({"text":value})),
            ));
        }
        return Ok(((*route).into(), Some(Value::Object(args.clone()))));
    }
    if name == "describe_table" {
        if !safe_identifier(value) {
            return Err(bad());
        }
        return Ok((format!("{route}{value}"), None));
    }
    if name == "read_knowledge" {
        if validate_path(value).is_err()
            || !value.is_ascii()
            || value
                .bytes()
                .any(|b| b <= 0x20 || b == 0x7f || b"%?#".contains(&b))
        {
            return Err(bad());
        }
        return Ok((format!("{route}{value}"), None));
    }
    Ok(((*route).into(), Some(Value::Object(args.clone()))))
}

fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.contains("..")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.".contains(&b))
}

fn empty_schema() -> Arc<JsonMap<String, Value>> {
    object_schema(JsonMap::new(), &[])
}

fn query_schema() -> Arc<JsonMap<String, Value>> {
    let mut sql = JsonMap::new();
    sql.insert("type".to_owned(), Value::String("string".to_owned()));
    let mut properties = JsonMap::new();
    properties.insert("sql".to_owned(), Value::Object(sql));
    object_schema(properties, &["sql"])
}

fn object_schema(
    properties: JsonMap<String, Value>,
    required: &[&str],
) -> Arc<JsonMap<String, Value>> {
    let mut schema = JsonMap::new();
    schema.insert("type".to_owned(), Value::String("object".to_owned()));
    schema.insert("additionalProperties".to_owned(), Value::Bool(false));
    schema.insert("properties".to_owned(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert(
            "required".to_owned(),
            Value::Array(
                required
                    .iter()
                    .map(|name| Value::String((*name).to_owned()))
                    .collect(),
            ),
        );
    }
    Arc::new(schema)
}

fn reject_cursor(method: &str, request: &Option<PaginatedRequestParams>) -> Result<(), McpError> {
    if request
        .as_ref()
        .and_then(|params| params.cursor.as_ref())
        .is_some()
    {
        log_status(method, 32602);
        return Err(McpError::invalid_params("pagination is not used", None));
    }
    Ok(())
}

fn reject_arguments(request: &CallToolRequestParams) -> Result<(), McpError> {
    if request
        .arguments
        .as_ref()
        .is_some_and(|arguments| !arguments.is_empty())
    {
        log_status("tools/call", 32602);
        return Err(McpError::invalid_params("arguments are not accepted", None));
    }
    Ok(())
}

fn required_sql(arguments: Option<&JsonMap<String, Value>>) -> Result<String, McpError> {
    let Some(arguments) = arguments else {
        log_status("tools/call", 32602);
        return Err(McpError::invalid_params("sql must be a string", None));
    };
    if arguments.len() != 1 {
        log_status("tools/call", 32602);
        return Err(McpError::invalid_params("sql must be a string", None));
    }
    match arguments.get("sql") {
        Some(Value::String(sql)) if !sql.is_empty() => Ok(sql.clone()),
        _ => {
            log_status("tools/call", 32602);
            Err(McpError::invalid_params("sql must be a string", None))
        }
    }
}

fn resources_from_pack(body: &Value) -> Result<Vec<Resource>, ()> {
    let files = body.get("files").and_then(Value::as_array).ok_or(())?;
    let mut resources = Vec::with_capacity(files.len());
    for file in files {
        let path = file.get("path").and_then(Value::as_str).ok_or(())?;
        validate_path(path)?;
        let mut resource = Resource::new(format!("oo://knowledge/{path}"), path)
            .with_mime_type("application/json");
        if let Some(bytes) = file.get("bytes").and_then(Value::as_u64) {
            resource = resource.with_size(bytes);
        }
        resources.push(resource);
    }
    Ok(resources)
}

fn file_object<'a>(body: &'a Value, path: &str) -> Option<&'a Value> {
    body.get("files")
        .and_then(Value::as_array)?
        .iter()
        .find(|file| file.get("path").and_then(Value::as_str) == Some(path))
}

pub(crate) fn knowledge_path(uri: &str) -> Result<&str, ()> {
    let path = uri.strip_prefix("oo://knowledge/").ok_or(())?;
    validate_path(path)?;
    Ok(path)
}

fn validate_path(path: &str) -> Result<(), ()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.contains("://")
    {
        return Err(());
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(());
    }
    Ok(())
}

fn log_status(method: &str, status: u16) {
    eprintln!("oo-mcp-gateway: method={method} status={status}");
}

#[cfg(test)]
mod tests {
    #[test]
    fn profile_queries_send_the_required_json_format() {
        assert_eq!(
            super::query_body("SELECT 1", true),
            serde_json::json!({"sql":"SELECT 1","format":"json"})
        );
        assert_eq!(
            super::query_body("SELECT 1", false),
            serde_json::json!({"sql":"SELECT 1"})
        );
    }

    use super::{knowledge_path, required_sql, tool_list, tool_request};
    use serde_json::json;

    #[test]
    fn each_semantic_tool_has_a_strict_gateway_contract() {
        for (name, args, route, post) in [
            ("list_tables", json!({}), "/v1/tables", false),
            (
                "describe_table",
                json!({"table":"demo.orders"}),
                "/v1/tables/demo.orders",
                false,
            ),
            (
                "resolve_entity",
                json!({"entity":"customer","text":"example"}),
                "/v1/entities/customer/resolve",
                true,
            ),
            (
                "search_values",
                json!({"column":"demo.orders.status","text":"shipped"}),
                "/v1/values/search",
                true,
            ),
            (
                "validate_sql",
                json!({"sql":"select 1"}),
                "/v1/sql/validate",
                true,
            ),
            ("get_freshness", json!({}), "/v1/freshness", false),
            (
                "read_knowledge",
                json!({"path":"entities/orders.md"}),
                "/v1/knowledge/entities/orders.md",
                false,
            ),
            (
                "search_knowledge",
                json!({"q":"orders"}),
                "/v1/knowledge/search",
                true,
            ),
            (
                "report_gap",
                json!({"description":"missing metric"}),
                "/v1/feedback/gap",
                true,
            ),
            ("check_readiness", json!({}), "/v1/check/readiness", false),
            (
                "check_answerable",
                json!({"question":"How many orders?"}),
                "/v1/check/answerable",
                true,
            ),
        ] {
            let (path, body) = tool_request(name, args.as_object()).unwrap();
            assert_eq!(path, route, "{name}");
            assert_eq!(body.is_some(), post, "{name}");
            let mut extra = args.as_object().unwrap().clone();
            extra.insert("role".into(), json!("administrator"));
            assert!(tool_request(name, Some(&extra)).is_err(), "{name}");
        }
        assert_eq!(
            required_sql(json!({"sql":"select 1"}).as_object()).unwrap(),
            "select 1"
        );
        assert!(required_sql(json!({"sql":"select 1","role":"admin"}).as_object()).is_err());
        for path in [
            "../private",
            "a%2fb",
            "a?role=admin",
            "a\r\nx:y",
            "/private",
        ] {
            assert!(tool_request("read_knowledge", json!({"path":path}).as_object()).is_err());
        }
    }

    #[test]
    fn knowledge_uris_stay_inside_the_pack_path() {
        assert_eq!(knowledge_path("oo://knowledge/README.md"), Ok("README.md"));
        assert_eq!(
            knowledge_path("oo://knowledge/entities/shop.md"),
            Ok("entities/shop.md")
        );
        assert!(knowledge_path("file:///tmp/secret").is_err());
        assert!(knowledge_path("oo://knowledge/../secret").is_err());
        assert!(knowledge_path("oo://knowledge//hidden").is_err());
        assert!(knowledge_path("/tmp/secret").is_err());
    }

    #[test]
    fn advertised_tools_include_preflight_and_semantic_routes() {
        let tools = tool_list();
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        assert_eq!(
            names,
            [
                "session_current",
                "pack_metadata",
                "query",
                "list_tables",
                "describe_table",
                "resolve_entity",
                "search_values",
                "validate_sql",
                "get_freshness",
                "read_knowledge",
                "search_knowledge",
                "report_gap",
                "check_readiness",
                "check_answerable"
            ]
        );
    }
}
