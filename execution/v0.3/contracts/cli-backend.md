# CLI delivery backend (V3-B)

The non-UI API is exported from `oo_cli`. Errors below are closed static codes,
never remote bodies or secret bytes.

- `profile::save(root: &Path, name: &str, profile: &serde_json::Value) -> Result<(), &'static str>`
  saves an already-issued credential **reference** under `profiles/<name>.json`
  (0600), then atomically selects it. Existing names are not overwritten.
  Required fields: `schema_version: 1`, HTTPS `gateway`, absolute `token_file`.
  Optional fields: `ca_file`, `client_pem`, `connect_ip`, `interface`,
  `business_timezone`, `identity`. The caller stores issued token bytes privately
  before calling this function; neither this function nor its return value prints them.
- `profile::select(root: &Path, name: &str)` and `profile::list(root: &Path)`
  implement `oo use NAME` and `oo profile list`. `active-profile.json` selects
  the named profile; absent a pointer, existing `config.json` is the legacy default.
- `check::readiness(profile: &Value) -> async Result<Value, &'static str>`
  reads the authenticated HTTPS `/v1/check/readiness` endpoint, which binds
  tables and freshness to one publication. `ready` requires a live publication
  and at least one visible table. It preserves source-time uncertainty and
  does not promise an atomic source-database snapshot.
- `mcp_config::render(agent: &str, profile: &str) -> Result<String, &'static str>`
  supports `claude`, `cursor` (JSON), `codex` (TOML). The command is
  `oo mcp --stdio --profile NAME`; no URL, token or CA bytes appear.
- `share_prompt::render(profile: &str) -> Result<String, &'static str>`
  returns a same-machine prompt with check, discovery and citation instructions.
  This prompt is not an enrollment grant and cannot authorize another machine.

Screen 5 integration (TUI lane): persist issued token, call `profile::save`,
render prompt into both text and `share_prompt` JSON field, optionally render
MCP config, then await readiness. Copy-to-clipboard and `m` key belong to TUI.
Do not display a green readiness result on a returned error or `ready: false`.
`oo init --profile` / post-publish token issuance remain owned by the init lane.

The MCP command requires the packaged `mcp-gateway` sibling. Release build
integration is supplied as `packages/cli/v03-release-integration.patch` for the
coordinator; no binary download or TLS bypass occurs on missing installations.
