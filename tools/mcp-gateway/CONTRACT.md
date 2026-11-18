# mcp-gateway contract

SP13-W01 stdio server for UUID `eef831aa-dba4-4af2-a78b-541bc9e0a388`. The frozen checklist is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a388/SP13/mcp-stdio/FROZEN-AC.md`.

The binary speaks MCP only through `rmcp` 3.5.0. OAuth and filesystem export
remain unsupported. The historical three-tool interface remains compatible
in explicitly configured loopback mode.

## v0.3 profile mode

`oo mcp --stdio --profile NAME` launches the colocated bridge, which delegates
each request over bounded stdin to the colocated native `oo mcp-request`.
Only a validated profile name and executable path travel in environment
variables. The native CLI loads private credential/CA references on every call,
verifies HTTPS, refuses redirects, and allowlists public semantic routes.
The npm launcher verifies both executables against its packaged manifest.
Changing the active profile does not change a running bridge's named profile.

Advertised tools: `list_tables`, `describe_table`, `resolve_entity`,
`search_values`, `query`, `validate_sql`, `get_freshness`, `read_knowledge`,
`search_knowledge`, `report_gap`, `check_readiness`, `check_answerable`.
The two historical metadata tools remain available. `report_gap` is neither
read-only nor idempotent; other tools do not write business data. The handshake
instructs callers to check, inspect published semantics, validate, then query.

Readiness binds tables and freshness to a publication and reports role-visible
table count. It does not assert source-real-time freshness.
Answerability is explicitly lexical-only, advisory and non-blocking. It never
promotes a table-name hit to `covered`. Entity, metric and code-value semantic
coverage, T6/demo recall >=95%, and false-pass <=5% are **not accepted**.

The current Gateway lacks `/v1/entities/{e}/resolve`, `/v1/values/search` and
`/v1/feedback/gap`. Their bridge contracts are implemented, but a missing
backend returns an error, not success or an empty result. Implementing those
Gateway routes is outside the B-lane endpoint write scope.

Release staging requires each target's prebuilt bridge and its source-license
inventory. The coordinator must apply `packages/cli/v03-release-integration.patch`
to build and collect these artifacts. Contract tests and local TLS fixtures
are not a real Agent/client or database-engine acceptance run.

## L5 Suggest mode (docs engine, B-S7-04 / B-S7-07)

Set `OO_MCP_DOCS_URL` (loopback `http://127.0.0.1:<port>`), `OO_MCP_DOCS_CREDENTIAL_FILE` (the share's `oodr_` credential, read on
every call) and `OO_MCP_DOCS_SHARE_ID`. The server then talks to the docs engine (`oo serve`) directly, never through the Gateway
data plane. With `OO_MCP_GATEWAY_URL` or a profile also set it advertises the Gateway tools **and** these; with the docs variables
alone it advertises only these (the historical interface above is unchanged when no docs variable is set).

- `suggest` (`readOnlyHint:false, destructiveHint:false, idempotentHint:true, openWorldHint:false`): `oo.suggest/v1` fields
  (`kind, subject, predicate, object|value, valid_from, supersedes, rationale, evidence[], idempotency_key`) become
  `POST /v1/docs/suggestions`. The share is the credential's: a `share_id` argument is refused before the call. The engine
  needs scope `suggest:write` (`DOCS_SUGGEST_SCOPE_MISSING` 403 comes back as an error result) and the share role
  `suggester` or `editor` (`DOCS_SUGGEST_ROLE_FORBIDDEN`). Resending the same `idempotency_key` changes nothing.
- `context_check` (read only): `POST /v1/docs/shares/<sid>:check?since=<seq>`; `exit_code` 0 continue, 10 refresh recommended,
  20 stop, 30 the share ended.
- Resource `puppyone://share/<sid>/updates` (only the configured share): `resources/read` returns the engine's update entries
  (`GET /shares/<sid>/updates`). `resources/subscribe` starts a watcher that polls the entries twice a second and sends
  `notifications/resources/updated` within 2 seconds of an appended entry (the legacy subscribe of protocol 2025-11-25; the new
  `subscriptions/listen` of 2026-07-28 is not served).
- The credential never appears in a process argument, an environment variable, a log line or a result.

## Unified ontology tools (D83)

`oo mcp --stdio --endpoint <url> --token-file <file>` starts the server with `OO_MCP_ONTOLOGY_URL` and `OO_MCP_ONTOLOGY_TOKEN_FILE`
(a loopback `oo serve` and an `ooa_` Agent token file). Four read-only tools, the same verbs as `oo ontology`, each a call of
`/v1/ontology/*` (docs-api 1.7) with the Agent token as the only bearer: `ontology_search {q, type?, limit?}`,
`ontology_entity {urn}`, `ontology_sources {}`, `ontology_query {sql, max_rows?}`. Every answer carries `provenance` (file anchor or
table row); scope, drive and revocation are decided by `oo serve` (revoked: 401 `ONTOLOGY_TOKEN_INVALID`; a write: 403
`ONTOLOGY_READ_ONLY`; no `db:read`: 403 `ONTOLOGY_SCOPE_DENIED`), returned as an error result, never as an empty one. The twelve
Gateway tools and the `--profile` mode are unchanged and are not advertised in this mode.
