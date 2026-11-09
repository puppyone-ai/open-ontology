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
