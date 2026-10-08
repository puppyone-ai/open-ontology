# SP13-W01 frozen acceptance — UUID eef831aa-dba4-4af2-a78b-541bc9e0a388

Frozen before product code. This card does not sign the SP13 parent, AT-MCP-01 parent acceptance, AT-MCP-02, AT-MCP-03, or remote OAuth.

## In scope

1. Official `rmcp` 3.5.0 (tag `rmcp-v3.5.0`) is the protocol implementation. A handwritten JSON-RPC stack is not acceptance.
2. Root `Cargo.toml` and root `Cargo.lock` stay unchanged. `rmcp` is admitted only by the child crate `tools/mcp-gateway` and its own `Cargo.lock`.
3. The production binary is a stdio MCP server. It wraps the existing Gateway HTTP routes and a compiled RolePack. It does not link `oo-gateway`, does not read pack files from disk, and does not fabricate rows, `query_id`, `warnings`, knowledge prose, or prompts.
4. Tools, exact and only:
   - `session_current` → `GET /v1/session/current`
   - `pack_metadata` → `GET /v1/pack/metadata`
   - `query` → `POST /v1/sql` with `{"sql": <string>}`
5. A non-2xx Gateway body is returned as MCP `structured_error` of that JSON. A 2xx body is `structured` content of that JSON. Field presence matches HTTP, including the absence of `query_id` and `warnings`.
6. Resources are `oo://knowledge/{path}` for paths in the pack `files` array only. `resources/read` returns that file object (`path`, `bytes`, `sha256`) as `application/json`. `file://`, absolute paths, and `..` are rejected. File prose is not read from disk.
7. `prompts/list` is empty. `export` is not a tool. Calling it is an unknown-tool protocol error and does not touch the filesystem.
8. Protocol support is exactly `2025-11-25` (initialize handshake) and `2026-07-28` (`server/discover` only). Initialize with `2026-07-28`, `2025-06-18`, or `1999-01-01` returns JSON-RPC `-32022`. The SDK fallback that accepts an unknown initialize version is overridden.
9. stderr is logs. stdout is protocol. Startup line: `oo-mcp-gateway: stdio ready oauth=false remote_export=disabled`. Per-call logs are method and status only.
10. Gateway URL is `OO_MCP_GATEWAY_URL` and must be `http://127.0.0.1:<port>` with no userinfo, query, fragment, or path. The bearer is read from `OO_MCP_TOKEN_FILE` on every call. The token is not an environment value and is not an argv value.
11. List and read results set `ttlMs` 0 and `cacheScope` `private`. The server keeps no pack cache. One process, with the token file swapped, must show the other role.
12. Live proof uses one private PostgreSQL 17.10, the existing `oo-gateway` binary, `role-pack-bundle` on the public compiler fixtures, and a real `rmcp` child process. Picker and auditor metadata differ. `unclassified_pii` is refused by the compiler and absent from both responses. A session token yields the Gateway 401 envelope. SQL yields the Gateway 501 envelope and does not echo the statement.

## Out of scope, not passed

- Remote Streamable HTTP, OAuth, RFC 9728, PKCE, and token exchange (SP13-W02 / AT-MCP-02 / AT-MCP-03).
- Docs/08 tools with no live Gateway route (`list_tables`, `describe_table`, `resolve_entity`, `search_values`, `validate_sql`, `get_freshness`, `read_knowledge`, `search_knowledge`, `report_gap`, local `export`).
- Parent or goal acceptance. The development ledger is not edited.

## Interfaces

| Item | Value |
| --- | --- |
| Binary | `mcp-gateway` (no argv) |
| Env | `OO_MCP_GATEWAY_URL`, `OO_MCP_TOKEN_FILE` |
| Startup stderr | `oo-mcp-gateway: stdio ready oauth=false remote_export=disabled` |
| Handshake version | `2025-11-25` only |
| Discover versions | `2025-11-25`, `2026-07-28` |
| Unknown initialize | `-32022` |
| Tool error from Gateway | `isError: true`, `structuredContent` = Gateway JSON |
| Resource MIME | `application/json` |
| OAuth flag | false, not implemented |
| Remote export | disabled, no tool |
