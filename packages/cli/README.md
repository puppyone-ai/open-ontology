# Open Ontology CLI

Standalone package `@reduxxx888/ontology-cli`, version `0.2.0`. Requires Node.js
22 or newer. The Node launcher verifies and executes the packaged Rust `oo`;
it does not download executables, install dependencies, or require a repository,
Rust toolchain, model API key, or direct database access.

Native query HTTPS uses the operating system's trusted CA store; the launcher
uses Node's trusted roots for authentication and version checks. Minimal Linux images must
provide a CA bundle, or the operator must configure an approved `--ca-file`.
The CLI never disables certificate or hostname verification to make a query work.

Release staging requires an `oo-lite-image` file next to each input native
binary. It contains the pinned lite runtime reference for that target's Docker
architecture (prefer `registry/repository@sha256:...`; a bare local image ID
requires an offline image archive). Staging copies it beside the installed
binary and refuses missing or unpinned sidecars. `NOTICE`,
`THIRD_PARTY_NOTICES.txt`, and `dependencies.json` are included in the npm pack;
source-notice collection is not legal clearance or a per-binary linkage audit.

## Install and configure

After the package has been published:

```sh
npm install --global @reduxxx888/ontology-cli@0.2.0
ontology --version
ontology configure --gateway https://gateway.example --token-stdin \
  --business-timezone Asia/Shanghai < /secure/gateway-token
```

Or reference an existing token file without copying its contents:

```sh
ontology configure --gateway https://gateway.example --token-file /secure/gateway-token
ontology config show --json
```

The package installs both `ontology` and `oo`, pointing to the same launcher.
An older unrelated `oo` earlier in PATH can shadow the npm command. Prefer
`ontology`, inspect `command -v oo` / `command -v ontology`, or invoke the installed
`<prefix>/bin/ontology` (local install: `node_modules/.bin/ontology`) explicitly.
Installing this package does not remove an unrelated old CLI.

Use a Gateway credential supplied by your operator, not a model API key. Never
put the token value in a command argument, shell history, package file, or npm
configuration. Redirect a protected file or pipe a secret manager's output.
Interactive terminal token input is rejected to avoid echoing credentials.

The profile is `~/.config/open-ontology/config.json`; set `OO_CONFIG_DIR` to use
another private directory. On POSIX the directory is mode `0700`, and managed
token files and the profile are `0600`. Keep the directory protected by the
current user's ACL on Windows; POSIX modes do not replace Windows ACLs.
The profile stores file references, never token contents. `config show` omits
credential/certificate paths and contents.

## Login, identity and sharing

```sh
oo login --gateway https://gateway.example --code-stdin \
  --business-timezone Asia/Shanghai < /secure/one-time-code
oo whoami --json
oo tables
```

Alternatively use `--code CODE` (visible in process arguments), or `--device`.
Prefer stdin for enrollment codes. Device login prints the verification address
and user code to stderr and polls until approval or expiry. Each `slow_down`
adds five seconds to all subsequent poll intervals; network errors back off.
TLS and hostname verification remain enabled, even when Node's TLS environment
variable asks to disable them. Redirects are never followed.

Login verifies identity and one real tables read before activating the profile.
It writes `tokens/default.token` with mode `0600` and saves only its reference
and identity metadata in `config.json`. Use `--profile NAME` for a different
token filename; login refuses to overwrite an existing name. The active
profile remains `config.json`, not a separate named-profile registry.
An unsuccessful read check leaves the active profile unchanged. A code may
already have been consumed when a later verification fails; request a new one.
`whoami` shows identity, role, expiry and protocol, warns on stderr at seven
days or less, and gives renewal/login guidance for expired or refused access.

Connected commands check anonymous `/v1/meta` before sending credentials.
Only protocol/minimum-version metadata is cached, for 24 hours, separately by
Gateway, routing and CA content. Exit `9` means the CLI is below the Gateway
minimum and includes a pinned npm upgrade command; exit `10` means an
unsupported protocol. Offline help, version and `agent-skill` stay offline.

Administrator operations require the already-approved mTLS identity and a
reviewed grant file with `project_id`, `principal_id`, `role`, `claims`,
`policy_version`, `knowledge_version`, `audience_url`, and optional `ttl_ms`
and `purpose`. The Gateway, not the launcher, validates grant authority.

```sh
oo token approve ABCD-1234 --gateway https://gateway.example \
  --client-pem /secure/admin.pem --grant-file /secure/reviewed-grant.json
oo share-prompt --gateway https://gateway.example \
  --client-pem /secure/admin.pem --grant-file /secure/reviewed-grant.json \
  --role analyst --agent codex
```

The default prompt contains a single-use enrollment code, its expiry, an exact
package version, verification commands and honest-query rules. `--raw-token`
instead issues a long-lived credential and adds a prominent risk warning.
Never store that output in public logs. Agent variants (`claude`, `cursor`,
`codex`) add setup instructions for the separate `tools/mcp-gateway` binary.
That existing bridge is loopback-HTTP-only: the prompt explicitly requires an
administrator-approved local endpoint and does not claim direct HTTPS support.

The launcher delegates `login`, `whoami` and `share-prompt` to the packaged
Rust binary, which also performs cached version negotiation for data commands.
`token approve` remains in the npm launcher. The HTTPS MCP bridge remains an
integration follow-up.

## Profile updates

Each configure operation replaces the profile rather than merging old settings.
Files are written atomically; a newly written token is referenced only after the
profile commits. Previous managed token files are retained for in-flight readers;
remove obsolete `token-*` files only after their readers have finished. External
token files are not modified. Failed configuration does not replace the profile.

Set `--business-timezone` explicitly for a business deployment. With that setting,
`oo freshness --summary --json` includes `business_time` alongside the unchanged
source freshness and warnings. This prevents a UTC host date from silently becoming
the business date. It is not an approved business-day or SLA calendar.

```sh
oo time --timezone Asia/Shanghai --at 2026-10-29T19:43:38Z --json
# today=2026-10-30; yesterday=[2026-10-29, 2026-10-30)
oo time --json  # use the configured business timezone
oo config set --business-timezone Asia/Shanghai  # preserve existing credentials and routing
```

Public HTTPS uses system certificate authorities, production transport, and
external authentication. It does **not** require an origin client certificate.
If your operator supplies them, configure optional routing or TLS files:

```sh
oo configure --gateway https://gateway.example --token-file /secure/gateway-token \
  --connect-ip 192.0.2.10 --interface en0 \
  --ca-file /secure/ca.pem --client-pem /secure/client.pem
```

Custom CA files replace system roots in the native CLI. `connect-ip` requires a
DNS hostname and preserves the gateway host and TLS verification. Use these
options only for the operator's intended route.

## Use directly

`oo --help` lists commands, connection options, query limits, search pagination
and exit codes. `oo help query` and `oo help knowledge search` are equivalent to
the respective `--help` commands. `oo configure --help` and `oo config show --help`
explain saved profiles without reading credentials or connecting. After configuring
once, humans and scripts use the same commands; a Skill or model is not required.

Long value options accept both `--name value` and `--name=value`. Boolean options
such as `--json` do not take a value. Use `--` before a positional value beginning
with `-`; SQL is one quoted argument, never a shell program.

`describe` shows a readable field/type table on a terminal. When piped or captured,
it preserves the legacy full JSON output, including scripts that omitted `--json`.
Use `oo describe --human app.orders` to request the compact readable view in an
Agent tool or pipe, and `--json` to explicitly request complete metadata anywhere.

## Errors and incomplete results

With `--json`, both launcher and native failures are structured JSON on stdout;
human-readable failures go to stderr. Errors include a class, nonzero exit status
and an actionable hint, without echoing credentials.

- `usage`: correct the named argument using `oo help <command>`; numeric options
  report their accepted range.
- `auth` / `UNAUTHENTICATED`: obtain a valid Gateway credential, not a model key.
- `transport` / `protocol`: investigate connectivity, TLS, proxy or version
  mismatch; these are not empty query results.
- `STATEMENT_NOT_ALLOWED` / `FUNCTION_NOT_ALLOWED`: inspect runtime capabilities
  and validate the SQL. These codes do not mean that a field is confidential.
- `TIMEOUT`: no complete result was delivered; inspect `oo status <query_id>`.
  Timeout requests are not blindly replayed.
- `truncated=true`: the response is an incomplete subset. Human output warns
  before the table; JSON retains the truncation reasons. Do not compute a global
  ranking from a truncated subset.

Data visibility is granted by the operator's published role/profile. A local
`configure` invocation cannot elevate access, write to the database, or bypass
another project's permissions.

```sh
ontology --help
ontology agent-skill
ontology agent-skill --json
oo freshness --summary --json
oo knowledge search --json "customer"
oo knowledge cat --json README.md
oo tables --json
oo describe --json app.orders
oo validate --json "SELECT COUNT(*) FROM app.orders"
oo query --json "SELECT COUNT(*) FROM app.orders"
```

Agents may load `oo agent-skill` for business-answer guidance, then use the same
installed commands to discover the actual schema and query the Gateway.
`app.orders` is an example, not a claim about your database. Help and agent-skill
are offline: no profile, credential, or connection is read, even if the saved
profile is malformed.
Use `ontology` in place of `oo` throughout these examples when avoiding an old
CLI in PATH. Top-level help identifies wrapper commands separately from native help.

All existing native commands, output, stdin and exit codes pass through. SQL is
one argument passed directly to the executable, never evaluated by a shell.
Native options use separate arguments, not `--flag=value`.
`numeric` and `bigint` cells are exact JSON strings; no SQL cast is needed to
preserve their precision. Unsupported SQL produces an error, never an empty
result. `validate` checks the same Gateway policy as `query`, but does not execute
the query or freeze the data.

Explicit native connection flags override the corresponding saved values.
`--token-stdin` replaces the saved token-file reference; explicit `--auth sandbox`
does not receive the saved external token. Other missing flags come from the
profile, including `--transport production` and `--auth external`. With no
profile, arguments go directly to the native CLI. Native validation and Gateway
authorization still apply; this wrapper grants no database permissions.

Changing `--gateway` to a different normalized origin or base path requires an
explicit `--token-file` or `--token-stdin`. The new destination receives no saved
token, CA file, client certificate, connect-IP or interface; supply any required
TLS/routing options explicitly. Repeating the same endpoint (including equivalent
host case, default port and trailing slash) retains the profile defaults.
Repeated `--gateway` options are rejected before a configured credential is attached.

## Packaged platforms and release boundary

The 0.2.0 release workflow builds `darwin-arm64`, `darwin-x64`, `linux-arm64`
and `linux-x64`. Linux uses a glibc 2.31 sysroot and runs smoke checks on
Debian 11 and Ubuntu 22.04; Darwin targets macOS 11 or newer. Darwin x64 is
cross-built in CI. Windows and musl/Alpine are not declared. Node.js 22 or
newer is required. These are build targets, not a publication or L1 acceptance claim.

Release staging requires all four targets declared in `package.json`.
For a local diagnostic install only, `stage.mjs --partial true` limits that
declaration to supplied binaries and marks the package private. Dependency
notice and binary checks still apply. Stage output must be outside Git or
inside a Git-ignored artifact directory such as `evidence/`.

The authoritative list is the keys of **this installed package's**
`native/manifest.json` under `binaries`. Do not infer platform support from
Node.js support or from another release. Each entry has this shape:

```json
{
  "schema_version": 1,
  "binaries": {
    "<platform>-<arch>": {
      "path": "native/<platform>-<arch>/oo",
      "sha256": "<64 hexadecimal characters>"
    }
  }
}
```

Windows entries use `oo.exe`. The launcher selects `process.platform` and
`process.arch` and verifies SHA256 before every execution. Missing targets,
files, or invalid digests fail explicitly. There is no PATH/global CLI fallback
and no install/postinstall download. The digest checks package consistency; it
is not a signature or a substitute for trusting the package publisher.

The npm allowlist includes only the launcher, native manifest and executables,
README, LICENSE, NOTICE, THIRD_PARTY_NOTICES.txt, dependencies.json and the staged
shared SKILL.md (plus npm's package.json).
Tests, build scripts, customer data, database exports, profiles, tokens, private
keys, and the rest of the repository are not release assets. Release staging
must supply binaries, manifest, licenses/notices, dependency inventory and SKILL.md,
then inspect the tarball before publishing. The dependency inventory records source
license evidence; it is not a legal clearance or an exact linked-symbol SBOM.

```sh
cargo build --locked -p oo-cli
npm test --prefix packages/cli
# Owned PostgreSQL 17 integration, from the repository root:
cargo build --locked -p oo-gateway --bins
python3 packages/cli/test/native-owned-pg.py evidence/<run-id>
```

Launcher tests use temporary fake executables and profiles; lifecycle tests
also exercise the built native binary against an owned HTTPS fixture. The
PostgreSQL runner reuses the Gateway lifecycle assertions with native
login/whoami/share-prompt/query invocations and records their counts. It creates
and stops its own PostgreSQL cluster and Gateway. These tests do not prove
cross-platform support, a real Agent session, or business-answer acceptance.

## Named local profiles and installation checks

`oo login --gateway URL --code-stdin --profile NAME --json` stores a named
profile and returns a credential-free, same-machine `share_prompt`.
Use `oo profile list`, `oo use NAME`, then `oo check --json`. An existing name
is never implicitly overwritten. Legacy `oo configure` still selects the
single `config.json` profile. Certificates stay local; no copying is needed
between a local CLI and its MCP bridge.

`oo mcp-config --agent claude|cursor|codex --profile NAME` renders configuration
containing only `oo mcp --stdio --profile NAME`. The prebuilt bridge must be
included by release staging. It uses the named profile, not whichever profile
is later selected with `oo use`.

After installation, run `oo install-check --json`. This offline check verifies
packaged executable hashes and distinguishes an optional packaged MySQL doctor
from the pinned Docker runtime fallback. `runtime_tested: false` is intentional:
it does not contact Docker or a database and is not an engine-support claim.
No npm lifecycle hook contacts the network or silently installs missing helpers.

Stack secret references additionally accept `env:VARIABLE` and
`file:/absolute/path`. External files must be owned by the user, be 0400 or
0600 regular single-link files, and have no symlink path components. Reads do
not chmod, copy, or print the file. Path-only consumers reject env references.
Container/SSH mount and environment propagation still require explicit
deployment integration; these forms do not silently grant a container access
to host files or environment variables.
