# agent-entry-v1

Owner `eef831aa-dba4-4af2-a78b-541bc9e0a397`. This file freezes the acceptance checks and the HTTP door before the Rust in this directory. `parent_acceptance` is false. This crate does not sign SP10, SP11, a parent ledger line, or the whole goal.

The listen profile `localprocess` is `DevelopmentLocalProcess`. It is a local process. It is not runsc and it is not a sandbox certificate. `production_signed` is always false. Profile `production` does not bind a socket. It exits 2. Missing runsc is `runsc_absent`. A runsc binary on disk is `production_driver_absent`. Neither case launches Pi. `pi-harness` `RunscApproval::Absent` and `RunscApproval::Presented` both refuse spawn.

## Producers

| Action | Producer | When the product route is absent |
|---|---|---|
| Management current | `control-identity` binary, `GET /control/v1/identity/current` | The action is not done. |
| Session create and close | `session-control-api` binary | The action is not done. |
| Model tool | Installed `oo` from `cargo build -p oo-cli --locked` | A hand-rolled client is not the SDK. |
| Pi | Pinned Pi 1.0.4 RPC through `tools/pi-harness` | A stub is not the turn. |
| `POST /v1/query` | this crate's fixed proxy, then the existing `query-pg` binary | Product `oo-gateway` HTTP has no such route. The proxy records mTLS `GET /health` (`sql_execution=unsupported`) and spawns `query-pg`. Label `product_http_sql=unsupported`. |
| Active role pack | `GET /control/v1/releases/active/pack`, operation `getActiveRolePack` | Product release-control-api is not edited. A private fixed producer in this crate reads a state file the create body does not write. |

A model HTTP 200 is recorded on the turn and does not set `complete`. `complete` is true only when the model’s own tool ran, a durable query-audit row exists, the answer cites that row’s `query_id`, and the audited cell matches the Rust gold. Pi `turn_finished` with `stopReason=stop` is event kind `model_stop`. It is not this contract’s `turn_finished`.

Stdout EOF, process death, and harness `incomplete` are `incomplete`. They do not emit `turn_finished`. A tool turn that ends without an audited `query_id` citation is `incomplete` reason `no-citation` and does not emit `turn_finished`. Cancel aborts the Pi child, records `incomplete` reason `cancelled`, leaves the session open, and does not emit `turn_finished`.

Close calls session-control-api `beginTurn` with the text `agent-entry-release-fixture` and then `cancel`. That beginTurn starts the session API’s `pg_sleep` fixture. It is not the model query and it is not the answer SQL. HTTP 200 from this crate requires the receipt `phase=closed`, `production_signed=false`, `deployment=localprocess`, eleven steps, and `s8` confirmed with `actual_absence=true`. Any other receipt, including HTTP 409, stays 409 and retryable. A 409 is not a closed session.

## Identity and the data role

The create body is not an identity and it is not a release. JSON fields `principal_id`, `project_id`, `role`, `claims`, `claims_hash`, `identity`, `pack_sha256`, `pack_digest`, `source_git_commit`, `content_hash`, `operation_id`, `policy`, and `knowledge` are `self_asserted_rejected` (422). `pack_mode=active_release` forwards the cookie `__Host-oo-session` to control-identity `GET /control/v1/identity/current` after the CSRF check. That GET rotates `csrf_token` and returns the next token. agent-entry first compares `X-OO-CSRF` with the session hash control-identity already stored for the cookie. A missing, malformed, or different token is HTTP 403 `csrf` and does not call current. A successful current adds response header `x-oo-csrf` with the rotated token. The next mutation must send that header value. `data_grants` must be `none`. `authority` must be `management_session`. `pack_mode=development_draft` does not call control-identity. It accepts `Authorization: Bearer` whose SHA-256 matches the map row `operator_token_sha256`. That token is not `public-proxy-marker` and is not the gateway credential. Pi's token file stays the public marker. An unknown draft bearer is HTTP 401 and inserts no query-audit row.

Management roles do not grant data. The operator map is an explicit row: subject, project, management role, data role, session principal. An admin subject with no row is HTTP 403 `data_role_unmapped`. That refusal creates no session and inserts no query-audit row.

Frozen rows used by the private proof:

| subject | management role | data role | session principal |
|---|---|---|---|
| `subwide` | `business_owner` | `oo_r_wide` | `wideowner` |
| `subnarrow` | `auditor` | `oo_r_narrow` | `narrowowner` |
| `subadmin` | `admin` | no row | refused |

Project id is `shop`.

The pack digest is SHA-256 over sorted lines `path<space>file-sha256\n`.

operationId: bindActiveReleasePack

`bindActiveReleasePack` reads the active release operation id, role, pack sha256, source git commit, policy, knowledge, and content hash from `getActiveRolePack`. The create body cannot supply `pack_sha256`, `pack_digest`, `source_git_commit`, `content_hash`, `operation_id`, `policy`, or `knowledge`. Those fields are `self_asserted_rejected` (422). agent-entry rehashes the role directory and requires equality with the returned pack sha256. `published` false, a missing active release, or a digest that was not returned by that GET is HTTP 409 `pack_unpublished`. createSession `pack_sha256` is that active digest. Each query reads the active pack again. The audit row stores that pack sha256. A revocation that changes the digest rolls the query back and inserts no audit row. A private directory whose digest was not the active release is not a publication.

## Roles and gold

Private database `oo_a397`. The shop formula is the published `testing/real-db/pg-e2e` formula, generated in this database. It is not a remount of a reviewed load.

- `shop.party` 2000 rows, index `i` in `0..2000`
- `shop.account` 8000 rows, index `j` in `0..8000`
- `shop.entry` 90000 rows, index `k` in `0..90000`

`oo_r_wide` may read every shop column except `national_id`. `oo_r_narrow` may read only `shop.party.party_key`, `shop.party.display_name`, `shop.account.account_id`, `shop.account.party_key`, `shop.account.opened`, `shop.entry.entry_id`, and `shop.entry.account_id`. `phone` is hidden from narrow. `national_id` is hidden from both. This is the pg-e2e column list, not `oo_r_restricted`.

Rust gold, checked against the loaded rows: `entry_count=90000`, `active_parties=1000`, party `p00000` has a null display name and `active=true`. The model prompt does not contain `90000`, `count(*)`, or `shop.entry`. The success answer cites a `query_id` whose audit row has result text `90000`.

A denied statement rolls back. It does not insert into `oo_agent_audit.query`. A clarification that does not call the tool inserts no query-audit row. A setup call made by the test harness is named `sdk_fixture_query` and is not the model completion.

## Query door

`oo query --json --auth external --token-file <0600> --gateway <http://127.0.0.1:port> --transport local-development` plus one SQL argument. The gateway origin does not already end in `/v1/query`. The token file is mode `0600`, not a symlink, and contains only the public marker `public-proxy-marker`. It is not a database password and it is not a reusable gateway session token. The proxy holds the query-pg password file and the mTLS client key.

The proxy accepts `Bearer public-proxy-marker`, strips that header, and does not forward it. One active turn selects the session. With no active turn, one session is used. With several sessions and no active turn, the sole `oo_r_wide` session is used. Any other case is HTTP 401 and writes no receipt. Two active turns are HTTP 409 and write no receipt.

`pack_mode=development_draft` binds the local role-directory digest. It is an explicit development preview, not an active production publication. A later digest mismatch is HTTP 409 and does not spawn `query-pg`. `pack_mode=active_release` still reads `getActiveRolePack` and refuses an unpublished or mismatched pack before `query-pg`.

Before `query-pg`, the proxy opens mTLS to the product `oo-gateway` and requires HTTP 200 from `GET /health` with `sql_execution=unsupported`. The hop record is status, byte count, and timing. It does not store certificates or secrets. A failed hop does not spawn `query-pg` and does not write a receipt.

`query-pg --request` is the existing guarded SELECT. This crate does not open a SQL driver for the model's statement and does not invent `_oo` rows. The request password stays in a mode `0600` file. The request text must not contain the password. Exit other than 0 writes no receipt. Exit 0 writes a receipt only when the query-pg JSONL audit contains `original_sha256`. `query_id` is `q_` plus the first 32 hex characters of that hash and matches `^q_[0-9A-Za-z]{6,40}$`. Cells are null, bool, or string. Counts are decimal strings. The response shape is the SDK `QueryResponse` with optional nulls omitted. Close removes the session from the proxy map only when the session receipt is closed.

## Model forwarder

Pi’s `apiKey` stays the public marker `public-proxy-marker`. This forwarder accepts only that bearer and substitutes the mode-`0600` upstream token toward the approved listener. The approved port is `8317`. Prefer `127.0.0.1:8317`. From a container whose loopback is closed, dial `host.docker.internal:8317` and record that path. The `Host` header sent upstream is `127.0.0.1:8317`. Also send `x-grok-client-version: 1.0.44` and `User-Agent: xai-grok-workspace/1.0.44`. Allow `GET /v1/models` and `POST /v1/chat/completions` only. Reject a JSON `model` other than `grok-4.7` before dialing. Do not rewrite the model and do not retry. Forward status, content type, and body, including SSE. The forwarder waits up to 900 seconds for one upstream response. The Pi turn deadline is 900 seconds. Drop the response if either token appears. Do not read auth stores or enterprise data. A 200 is not completion.

## Pi pin

Version `1.0.4`. Git `7c10bd4337495ee613f2224843ecdf349b80d1df`. Node `v22.23.3` linux-arm64. The proof checks the npm tarball SHA-256 `04910bdae661a6529e9d6869b04006f6aad01186398b04a16c6d9793667b96c5`, the Node tarball SHA-256 `5ced2d48d1d7198739b7f86804de0171aefb6823b684b12341d3321afc3cb0b2`, and `pi --version` stdout `1.0.4`. Launch is `--mode rpc` with `--no-extensions --no-mcp --no-skills`. The child `PATH` is only the Node directory, so the prompt passes the absolute `oo` path. The proof may symlink `bash` into that directory so the shell resolves. Dev launch passes `LocalProcessNotSandbox`.

## HTTP

Listen `127.0.0.1` only. JSON only. `pack_mode=active_release` mutations require the identity cookie and `X-OO-CSRF`. `pack_mode=development_draft` mutations require `Authorization: Bearer` of the locally registered operator token. The draft path does not send `x-oo-csrf` and does not set an authenticated flag.

### `POST /entry/v1/sessions`

201 body: `contract=agent-entry-v1`, `operation=createSession`, `session_id`, `project_id`, `management_role`, `data_role`, `pack_sha256`, `deployment=localprocess`, `production_signed=false`, `runsc=absent_development_local_process`, `conversation_id`, `parent_acceptance=false`, `runtime=development_local_process`, `sandbox_certificate=false`.

401 `unauthenticated`. 403 `data_role_unmapped` or `csrf`. 409 `pack_unpublished`. 422 `self_asserted_rejected` or `malformed`.

### `POST /entry/v1/sessions/{id}/turns`

Body is `{"text":"<natural language>"}` only. 202 means Pi accepted the prompt. It does not mean the turn finished. Body: `operation=beginTurn`, `turn_id`, `status=accepted`, `model_http_status=null`, `complete=false`, `harness_pid`.

### `GET /entry/v1/sessions/{id}/events?after=N`

Sequences are contiguous from 1. `after` greater than the latest sequence is 409 `gap`. Kinds: `turn_started`, `assistant_delta`, `tool_call_started`, `tool_call_finished`, `citation`, `clarification`, `incomplete`, `model_stop`, `error`, `turn_finished`. `turn_finished` is emitted only after citation and gold.

### `POST /entry/v1/sessions/{id}/cancel`

200. `operation=cancelSession`, `status=incomplete`, `reason=cancelled`, `turn_finished=false`.

### `POST /entry/v1/sessions/{id}/close`

200 only for a real closed receipt. Otherwise 409. `parent_acceptance=false`.

## Draft development preview

`specs/minimal-product-loop-plan.md` is the current milestone order. This crate does not sign that plan, OC-01 through OC-06, or a production publication. Profile `localprocess` is an explicit development process. It is not runsc.

Pi uses the pinned RPC process and its built-in read and bash tools. The launch passes `--no-extensions --no-mcp --no-skills`. MCP is not a prerequisite. The model writes the SQL. The prompt does not contain the answer SQL.

`pack_mode=development_draft` is the explicit single-user preview. Management roles still do not grant data. On `active_release`, an admin subject with no map row is still HTTP 403. The draft digest is the local directory hash, not a caller-supplied digest, and it is not an active production publication. `pack_mode=active_release` remains the `bindActiveReleasePack` path and is not signed here as OC-05 closure.

`development_draft` requires `import_generation_file`, an absolute path. A private fixture file is `{"contract":"agent-entry-import-generation","source":"private_fixture","mp01":false,"generation":"<64 lowercase hex>","database":"oo_a397"}`. That file is not MP-01a–d. A sealed local import is the same contract with `source=local_import`, `mp01=true`, `generation` equal to the manifest `import_hash`, `manifest_path`, `target_config_path`, `mapping_source`, and `config_digest`. Startup recomputes the sealed-import hash and the manifest body sha256, then requires the target file bytes to hash to `target_config_sha256`. The target protocol is `oo.pipeline.local-pg-mirror-target/v1`, `tls` is `verify-full`, and `host` equals `server_name`. The file database must equal that target database and the configured local database. `mapping_source` and `config_digest` are the `_oo.physical_mappings` identity for that copy. `mp01` is provenance only and does not accept MP-01a–d. `source_write_tuples` in the manifest is not a proof that the source received no writes. Any other shape, including `mp01=true` without that manifest and target file, refuses startup. The source secret is not in the file and is not passed to Pi. A development_draft query receipt stores `import_generation`, `import_source`, `mp01`, `snapshot_id`, and `import_generation_number`. A sealed receipt also stores `plan_sha256`, `manifest_sha256`, `target_config_sha256`, `target_host`, `target_server_name`, `target_user`, `mapping_source`, `config_digest`, and `preview=explicit_local_preview`. A private fixture receipt has an empty snapshot and a null generation number. A live proof that misses the binary, the operator auth, the model turn, the tool call, the query, the audit row, or close is a failed proof.

Before `query-pg`, a `local_import` policy must be an explicit local preview. The endpoint host, database, and user must equal the sealed target identity. `identity.project` and every policy `project` must equal `mapping_source`. `policy_id` and every `config_digest` must equal the file digest. `generation` must equal the manifest `import_generation`. Each selected column must be an original manifest column whose oid maps to `bool`, `int2`, `int4`, `int8`, `numeric` (precision 1 through 38), or `text`, and the policy `semantic_type` must be that name. Column `status` must be `auto_accepted`. `confirmed` is refused as `preview_not_approval`. A published, approved, or human_approved key is refused. `pii.class` must be `none`. `catalog.unknown_rows` must be `refuse`. A mismatch is HTTP 409 `PREVIEW_POLICY` and does not spawn `query-pg`. This preview is not an active production publication and is not a human approval.

`query-pg` still requires a `NOLOGIN` database role named `oo_r_` plus the policy slug, with column `SELECT` on the mapped physical columns, and membership that lets the login `SET ROLE` to it. Reading `_oo.physical_mappings` and executing `_oo.lookup_physical_mapping` does not create that role. This crate does not `GRANT`, `CREATE ROLE`, or otherwise alter the shared target.

Product `oo-gateway` HTTP still has no `POST /v1/query`. Its health body reports `sql_execution=unsupported`. The existing `query-pg` command is the guarded SELECT. It loads `_oo.physical_mappings`. This crate does not invent that catalog and does not replace `query-pg` with its own SQL execution. A missing mapping is a `query-pg` refusal and writes no receipt. That refusal is not a completed count.

## Out of scope

No edits to root manifests, core, gateway, pi-harness, session crates, control-identity, release-control-api, or frontend. No pre-written answer SQL. No ontology-dialogue proposal. No customer IdP. No runsc certificate.
