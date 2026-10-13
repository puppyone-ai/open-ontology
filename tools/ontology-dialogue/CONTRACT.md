# Ontology dialogue

Contract `oo.ontology-dialogue/v1`. Binary `ontology-dialogue`.

This is one trusted-operator command on this machine. It has two phases, `propose` and `execute`. `propose` sends one natural-language question and one role Pack's public files to one approved OpenAI-compatible chat completion. The default transport is production HTTPS. `execute` runs only after a separate local approval file names the exact proposal hash, SQL hash, tuple hash, and Pack digest. The model SQL is untrusted. This command never calls `execute_trusted_select`. It never starts `query-pg` from `propose`. It never treats the approval file as authenticated authentication.

`query-pg` remains the local operator CLI in `contracts/gateway/query-pg-cli-v1.md`. Its request file is the operator document that CLI already trusts. Unknown parser-worker admission stays blocked inside that CLI. This command does not open that admission. Full hosted Gateway authentication, `runsc`, and the natural-language harness stay parent work.

The citation object is the `answer-citations` module report (`tools/answer-citations`, `validate`). `authenticated_audit` stays false. `entailment` and `warning_comprehension` stay `not_judged`. This command does not claim the audit names a session principal, and it does not claim the answer text follows from the rows or explains a truncation.

## Usage

```text
ontology-dialogue propose --config <path>
ontology-dialogue execute --config <path>
ontology-dialogue --help
```

`--help` alone exits 0 and prints the operator summary. Any other arguments exit 2. Stderr is `ontology-dialogue: ` plus one static token. The token is never the question, the SQL, a cell, the bearer, a password, or a path.

`propose` and `execute` read one JSON config file. The bearer, the query password, and the CA are not arguments and are not environment variables. Success writes one JSON object and one trailing newline to stdout. A fault before a proposal or before the query child writes no stdout.

## Shared limits

Configs, the question file, the approval file, and the proposal file are at most 65536 bytes. The Pack artifact is at most 1048576 bytes. Each file this command reads is a regular non-symlink. A symlink is refused before it is followed. JSON is UTF-8. The parser rejects a leading BOM, duplicate keys, trailing bytes, depth above 16, and more than 8192 JSON nodes. `serde` then rejects unknown fields on this command's own documents. JSON null is rejected in those documents. `serde_json` is built with `arbitrary_precision`.

Hashes are SHA-256, 64 lowercase hex digits, of the exact UTF-8 bytes named below. Canonical JSON sorts object keys by UTF-8 byte order, keeps array order, and escapes controls, quotes, and backslashes. It does not escape solidus.

## Propose config

`protocol` is `oo.ontology-dialogue.propose-config/v1`.

| Field | Rule |
|---|---|
| `question_file` | `oo.ontology-dialogue.question/v1` with `text` |
| `pack_file` | one `oo.builder.role-pack/v1` artifact |
| `binding.role` | `oo_r_` plus a slug, length at most 63 |
| `binding.project` | slug, 1 to 63 bytes |
| `binding.policy_id` | 64 lowercase hex digits, the Gateway request config digest |
| `binding.policy_version` | 1 to 64 bytes of ASCII letters, digits, `.`, `_`, `+`, `-` |
| `binding.knowledge_id` | role-pack knowledge slug, 1 to 63 bytes |
| `binding.knowledge_version` | same rule as `policy_version` |
| `binding.as_of` | `YYYY-MM-DDThh:mm:ssZ` |
| `endpoint.transport` | omitted or `production`, or the exact token `development_local` |
| `endpoint.origin` | production: `https` URL below. `development_local`: `http://127.0.0.1:<port>` below |
| `endpoint.path` | absolute path, 1 to 256 bytes, no `//`, `..`, `\`, `?`, or `#` |
| `endpoint.model_id` | 1 to 128 bytes of ASCII letters, digits, `.`, `_`, `+`, `-` |
| `trust.ca_file` | production: PEM bundle below. `development_local`: present in JSON and not read |
| `trust.residency_approved` | boolean `true` |
| `secret.bearer_file` | bearer token file, at most 512 bytes |
| `budget` | the ranges below |

`binding.project` uses a dialogue slug: one ASCII lowercase letter or `_`, then lowercase letters, digits, or `_`, length 1 to 63. `binding.role` is `oo_r_` plus that slug. The whole role is at most 63 bytes, and the part after `oo_r_` is 1 to 58 bytes. A hyphen stays outside the project and role tokens.

`binding.knowledge_id` uses the role-pack knowledge slug from `contracts/builder/role-pack-v1.md`. The length is 1 to 63. The first byte is an ASCII lowercase letter. Later bytes are lowercase letters, digits, `_`, or `-`. `know-a317` and `know_a317` are both legal. This command copies the id the compiler emitted. It does not rewrite it. A leading `_` or `-`, uppercase, a length outside 1 to 63, `/`, a space, a newline, or a control character is `config` before any socket.

`binding.policy_id` is 64 lowercase hex digits. That value is the digest the role-pack compiler copied from its policy id parameter into the pack manifest, and the digest `query-pg` requires on `identity.policy_id`. Uppercase hex, any other length, a non-hex byte, a slash, or a control character is `config`. The role-pack compiler also accepts its existing slug as a policy id and does not rewrite a digest. A slug-only policy id can still be a legal role pack. This command accepts the digest form in `binding.policy_id`. The loaded pack's `policy.id` must be that digest, and its `knowledge.id` must be the knowledge slug above.

`residency_approved: true` is the caller's declaration that this origin is approved. It is not a residency certificate. On the production client the CA file is not a private key. A PEM window containing `PRIVATE KEY` is `config`. That client trusts that bundle and no platform roots.

The bearer is the file bytes without one optional trailing newline. It is visible ASCII (`0x21` through `0x7e`) and contains no whitespace. Production length is 16 to 256 bytes. `development_local` also accepts a length of exactly 6 bytes. Lengths 1 to 5 and 7 to 15 are `config` in both modes. If the bearer bytes occur in the config, the question file, the Pack bytes, or the chat body, the call is `secret` and no socket is opened. The token is placed only in the `Authorization` header.

### Development local

`endpoint.transport` must be the exact string `development_local`. The origin must be `http://127.0.0.1:<port>`: port 1 to 65535, one to five digits, no leading zero, no user, password, path, query, or fragment. `localhost`, any other host, `https`, and a trailing slash are `config`. This mode uses a separate HTTP client with `https_only(false)`, no CA bundle, and no platform roots. The production client is unchanged: `https_only(true)` and `tls_certs_only` on the pinned bundle. An omitted transport and the token `production` both use that production client. Any other transport token is `config`.

There is no field that names `query-pg`, a database password, a source catalog, or a second Pack.

### Budget

A number outside the range is `budget`.

| Field | Range |
|---|---|
| `max_question_bytes` | 1 to 8192 |
| `max_context_bytes` | 1 to 65536 |
| `max_request_bytes` | 1 to 65536 |
| `max_response_bytes` | 1 to 16384 |
| `max_response_chunks` | 1 to 32 |
| `deadline_ms` | 1 to 30000 |
| `max_prompt_tokens` | 1 to 100000 |
| `max_completion_tokens` | 1 to 4096 |
| `max_total_tokens` | 1 to 100000 |
| `max_sql_bytes` | 1 to 65536 |

`deadline_ms` bounds the one model call from the start of the connection attempt through the last accepted body chunk.

## Pack and public context

The Pack is the role-pack artifact. This command does not compile it and does not read any other role's Pack. It recomputes each file SHA-256, `manifest_sha256`, and `pack_digest`. A mismatch is `pack`.

`pack_digest` is the SHA-256 of the canonical JSON array of `{bytes, path, sha256}` for every file, including `manifest.json`, in path order. `bytes` is the UTF-8 length of `text`. `manifest.json` must be one file. Its `protocol` is `oo.builder.role-pack-manifest/v1`. The manifest's `project`, `role`, `policy.id`, `policy.version`, `knowledge.id`, and `knowledge.version` must equal the binding. Other manifest fields are not interpreted. The artifact `protocol` is `oo.builder.role-pack/v1` and `compiler` is `oo-builder/role-pack/v1`. At most 128 files. Paths are relative, NFC is not rewritten, and an empty path, `\`, `:`, a leading `/`, `//`, or a `.` or `..` segment is `pack`.

File text that contains `secret://`, `postgres://`, `mysql://`, or `password=` is `pack` and is not sent. The public context is every remaining file, in path order. The model message is only that context plus the question text. It does not include the bearer, the query request, a password file, a control config, a raw source catalog, or another Pack.

## Model call

One POST to `origin` plus `path`. reqwest `0.13.5` with `default-features = false` and features `rustls` and `http2`. Both clients use `redirect(Policy::none)`, `retry(retry::never())`, `no_proxy`, `http1_only`, `referer(false)`, and `connection_verbose(false)`. The production client also uses `https_only(true)` and `tls_certs_only`. The `development_local` client uses `https_only(false)` and does not install a CA or platform roots. The body is read with `Response::chunk` under the chunk and byte caps. The client does not read `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `OPENAI_API_KEY`, or `OPENAI_BASE_URL`. There is one attempt and one URL. A redirect is not followed. A non-200 status, including HTTP 500, is `provider` and writes no proposal.

The chat body is one JSON object: `model`, `temperature` `0`, `stream` `false`, `max_completion_tokens`, two messages, and `response_format.type` `json_schema`. The system message is the fixed sentence in the source constant `SYSTEM_PROMPT`. The user message is `question:\n`, the question text, `\n\ncontext:\n`, then each public file as `path\n` plus its text and `\n`. No second message is added.

A completion is accepted only when the HTTP status is 200, there is one choice, `finish_reason` is `stop`, usage is present, prompt plus completion equals total, usage is inside the token caps, and the assistant content is one object `{"protocol":"oo.ontology-dialogue.sql-candidate/v1","sql":"..."}` with no other keys. `sql` is 1 to `max_sql_bytes` UTF-8 bytes and contains no NUL. Any other completion, including `finish_reason` `length`, a non-200 status, a timeout, a redirect, or a bearer echo, is a provider or budget fault and writes no proposal.

## Proposal

Stdout is one object plus a newline. `model_sql` is `untrusted`. `auto_execute` is false. `authenticated` is false. `trust` is `local_operator_declared`.

| Field | Bytes that were hashed |
|---|---|
| `question_sha256` | the question file |
| `sql_sha256` | the SQL string, with no added newline |
| `tuple_sha256` | the tuple line below |
| `proposal_sha256` | the canonical proposal preimage below |
| `pack_digest` | the Pack digest defined above |
| `manifest_sha256` | the `manifest.json` text |

The tuple line is one UTF-8 line, with no newline:

```text
oo.ontology-dialogue.tuple/v1|role=<role>|project=<project>|policy=<policy_id>@<policy_version>|knowledge=<knowledge_id>@<knowledge_version>|as_of=<as_of>|pack=<pack_digest>
```

The proposal preimage is the canonical object of `as_of`, `knowledge_id`, `knowledge_version`, `manifest_sha256`, `pack_digest`, `policy_id`, `policy_version`, `project`, `question_sha256`, `role`, `sql_sha256`, and `tuple_sha256`. It does not contain the SQL text. The approval therefore names both the proposal hash and the SQL hash.

## Execute config

`protocol` is `oo.ontology-dialogue.execute-config/v1`.

| Field | Rule |
|---|---|
| `proposal_file` | the proposal object |
| `approval_file` | the approval object |
| `pack_file` | the current Pack artifact |
| `request_file` | one existing `oo.gateway.query-pg/v1` document whose `statement.source` is `request` |
| `query_pg.path` | the `query-pg` binary |
| `query_pg.sha256` | SHA-256 of that binary |
| `binding.session_id` | caller label, 1 to 64 bytes, first byte a letter or digit, then letters, digits, `_`, or `-` |
| `binding.turn_id` | the same label rule |
| `binding.declares_data` | boolean passed to `answer-citations` |
| `binding.render_rows` | boolean |
| `budget.deadline_ms` | 1 to 30000, the child wall clock |
| `budget.max_stdout_bytes` | 1 to 1048576, bytes kept from the child stdout |

The session and turn labels are caller text. They are not read from the audit receipt. There is no principal field and no field that marks the approval as authenticated.

## Approval

`protocol` is `oo.ontology-dialogue.approval/v1`. `trust` is `local_operator_declared`. `authenticated` must be false. Any other boolean is `approval`. The file names `proposal_sha256`, `sql_sha256`, `tuple_sha256`, and `pack_digest`.

Before the `query-pg` binary is opened or spawned, `execute` recomputes the current Pack digest and the tuple from the current Pack and the proposal binding. It recomputes `sql_sha256` from the proposal SQL and `proposal_sha256` from the preimage. The approval must equal those current values, and the proposal's own hash fields must equal them too. The request's `statement.sql` must equal the proposal SQL. The request's `identity.role`, `identity.project`, and `identity.policy_id` must equal the proposal. A missing file, a changed hash, a stale Pack, or a tampered SQL is `approval`. The child is not spawned.

This check is the operator's declared review. It is not a signature, a login, or Gateway authentication.

The binary is hashed after that check. A mismatch is `binary` and the child is not spawned. The spawn is `query-pg --request <path>` with a cleared environment, `PATH=/usr/bin:/bin`, and no stdin. The password file named by the request is not opened by this command. Child stdout and stderr are not copied into this command's stderr. A known `query-pg: <token>` line may be stored as `child_token`. Any other stderr is dropped.

`deadline_ms` is the child wall clock. On expiry the child is killed and the result is `query`.

## Result

The execution object is written when the child was spawned. Key order:

| Field | Value |
|---|---|
| `protocol` | `oo.ontology-dialogue.execution/v1` |
| `trust` | `local_operator_declared` |
| `authenticated_audit` | false |
| `entailment` | `not_judged` |
| `warning_comprehension` | `not_judged` |
| `auto_execute` | false |
| `spawned` | true |
| `child_exit` | the child status code, or null if the child was killed |
| `child_token` | a known token, or null |
| `query_receipt` | `clean`, `truncated`, `failed`, `absent`, `unknown`, or `incomplete` |
| `query_id` | the one admitted id from the citation report, or null |
| `sql_sha256` | the approved SQL hash |
| `proposal_sha256` | the approved proposal hash |
| `summary` | the one `query-pg` summary object, or null |
| `rows` | the typed rows when `render_rows` is true, otherwise null |
| `rendered` | the exact answer text submitted to `answer-citations`, or null when there is no receipt |
| `citation` | the `answer-citations` report object, or null when there is no receipt |

`query_id` is copied only from `citation.admitted` when that array has one object and `citation_status` is `ok`. It is not minted from a ULID. The answer text may contain `[q_...]` only by the answer-citations rule: `q_` plus a receipt fingerprint of 6 to 40 lowercase hex digits. A failed, truncated, unknown, or incomplete receipt stays that class. A guard refusal creates no audit file, so `query_receipt` is `absent`, `citation` is null, and `query_id` is null.

`clean` means the child exited 0, the summary status is `succeeded`, `truncated` is false, `end_audit_ok` is true, and the citation report is `ok`. `truncated` means the same with `truncated` true and a `querytruncated` disclosure present. `failed` means a receipt was read and the summary status is `failed` or the child reported a query failure. `absent` means the child produced no audit file. `unknown` means the file or the stdout was not a single producer result. `incomplete` means the citation report classified the cited id as `incomplete`.

When `render_rows` is true, each row is the `query-pg` `cells` array in order. Kinds stay `null`, `bool`, `int`, and `text`. `int` and `text` stay JSON strings. Nothing is reformatted. The rendered answer lists `status`, `truncated`, then `cell <kind> <value>` lines when rendering, then the citation brackets. That text is not a second model call.

The citation request uses `trust` `local_operator_declared`, the caller session and turn, the proposal role and project, protocol `oo.gateway.query-pg-audit/v1`, and the audit file bytes with their SHA-256. The report is embedded as the module serialized it.

## Exits

| Code | Token | When |
|---|---|---|
| 0 | | `propose` wrote a proposal, or `execute` spawned a child whose summary is a clean or disclosed truncated success and whose citation report is `ok`. |
| 1 | `provider` | The model call failed, timed out, redirected, or returned a body that is not one SQL candidate. No proposal is written. |
| 1 | `secret` | The bearer occurs in a local document or in the response. |
| 1 | `budget` | A context, request, response, or token cap failed. |
| 1 | `io` | A local read or write failed after the document was accepted. |
| 2 | `usage` | The arguments are not the usage above. |
| 3 | `config` | The config, question, endpoint, CA, or bearer file is refused. |
| 4 | `pack` | The Pack digest, manifest, or binding does not match. |
| 4 | `approval` | The approval is missing, stale, or tampered. The child was not spawned. |
| 5 | `binary` | The `query-pg` hash does not match. The child was not spawned. |
| 6 | `guard` | The child exited 6. No audit file is expected. No `query_id` is written. |
| 7 | `query` | The child reported a query, policy, TLS, audit, or timeout failure, or the receipt is failed or unknown. |
| 8 | `citations` | A receipt was checked and the citation report is `invalid`. |

Stdout on exits 6, 7, and 8 is the execution object. Stdout on exits 1 through 5 is empty, except exit 0.

## Out of scope

This command does not install grants, onboard a database, compile a Pack, authenticate an operator, admit an untrusted parser worker, start `runsc`, call a second model to phrase an answer, or retry a provider. Database preparation in the proof uses the compiled `provision-mapping-reader`, `install-column-acl`, and `query-pg` binaries against a private PostgreSQL. The `pg-onboarding` orchestrator remains the parent sequence that pins those children. This command does not replace it.
