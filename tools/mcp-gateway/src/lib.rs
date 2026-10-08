//! Stdio MCP server for the existing loopback Gateway.
//!
//! Protocol handling is `rmcp` 3.5.0. This crate supplies the Gateway calls,
//! the strict version gate, and the refusal of remote export.

#![forbid(unsafe_code)]

mod http;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use http::{parse_loopback_port, GatewayClient};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, CacheScope, DiscoverResult, ErrorCode,
    Implementation, InitializeRequestParams, InitializeResult, ListPromptsResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource, ResourceContents,
    ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt};
use serde_json::{Map as JsonMap, Value};

const SUPPORTED: &[ProtocolVersion] = &[
    ProtocolVersion::V_2025_11_25,
    ProtocolVersion::V_2026_07_28,
];

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

    async fn gateway_tool(&self, path: &str, body: Option<Value>) -> Result<CallToolResult, McpError> {
        let reply = match body {
            None => self.client.get(path).await,
            Some(body) => self.client.post_json(path, &body).await,
        };
        match reply {
            Ok(reply) => {
                log_status("tools/call", reply.status);
                if (200..300).contains(&reply.status) {
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
                self.gateway_tool("/v1/sql", Some(serde_json::json!({ "sql": sql })))
                    .await
                    .map(CallToolResponse::from)
            }
            _ => {
                log_status("tools/call", 32601);
                Err(McpError::new(ErrorCode::METHOD_NOT_FOUND, "unknown tool", None))
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
        "Loopback gateway tools only. OAuth is not implemented. Remote filesystem export is disabled.",
    )
}

fn tool_list() -> Vec<Tool> {
    let annotations = ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false);
    vec![
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
            "POST /v1/sql. Returns the gateway JSON body.",
            query_schema(),
        )
        .with_annotations(annotations),
    ]
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

fn object_schema(properties: JsonMap<String, Value>, required: &[&str]) -> Arc<JsonMap<String, Value>> {
    let mut schema = JsonMap::new();
    schema.insert("type".to_owned(), Value::String("object".to_owned()));
    schema.insert("additionalProperties".to_owned(), Value::Bool(false));
    schema.insert("properties".to_owned(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert(
            "required".to_owned(),
            Value::Array(required.iter().map(|name| Value::String((*name).to_owned())).collect()),
        );
    }
    Arc::new(schema)
}

fn reject_cursor(method: &str, request: &Option<PaginatedRequestParams>) -> Result<(), McpError> {
    if request.as_ref().and_then(|params| params.cursor.as_ref()).is_some() {
        log_status(method, 32602);
        return Err(McpError::invalid_params("pagination is not used", None));
    }
    Ok(())
}

fn reject_arguments(request: &CallToolRequestParams) -> Result<(), McpError> {
    if request.arguments.as_ref().is_some_and(|arguments| !arguments.is_empty()) {
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
    if path.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..") {
        return Err(());
    }
    Ok(())
}

fn log_status(method: &str, status: u16) {
    eprintln!("oo-mcp-gateway: method={method} status={status}");
}

#[cfg(test)]
mod tests {
    use super::{knowledge_path, tool_list};

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
    fn advertised_tools_are_the_three_gateway_routes() {
        let tools = tool_list();
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        assert_eq!(names, ["session_current", "pack_metadata", "query"]);
    }
}
