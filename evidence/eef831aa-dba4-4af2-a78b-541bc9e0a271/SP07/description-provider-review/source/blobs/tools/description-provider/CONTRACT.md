# Description provider

Contract `oo.builder.description-provider/v1`. Compiler id `oo-builder/description-provider/v1`.

This unit is one HTTPS call to one caller-approved OpenAI-compatible chat-completion endpoint. The call carries the description request produced by `prepare_description_request`. The model content is passed to `complete_description_candidates` only when the completion is whole, in budget, and free of the bearer secret. Those two functions are the description-candidate API. The `description-candidate` CLI calls the same functions. This unit does not edit that compiler.

A successful candidate has `effect=review_only`. This unit does not write curated or effective text, grant, certify, classify PII, publish, or emit a billing certificate. It does not cache, retry, stream without a bound, or fall through to a second endpoint. Residency of a real provider, a real owner approval, a real invoice, and full S5 stay later gates.

Fixed producer `182e2137ee261ed889b1aff5ada6e505a4a17723`. The known `selected_datasets` order is that producer's behavior. This unit does not repair it and does not accept S5 because of it.

## Config

`description-provider run --config PATH` reads one JSON object from `PATH` and one `oo.builder.description-request-command/v1` document from stdin. The bearer token is not an argument, not an environment variable, and not a field of this object. Stdin is at most 1048576 bytes. The config file is at most 65536 bytes. A longer read fails before a parse. Messages are static.

| Field | Rule |
|---|---|
| `protocol` | `oo.builder.description-provider-config/v1` |
| `endpoint.origin` | `https` URL, host present, no user, password, query, fragment, or path other than `/` |
| `endpoint.path` | absolute path, 1 to 256 bytes, no `//`, `..`, `\`, `?`, or `#` |
| `endpoint.model_id` | 1 to 128 bytes of ASCII letters, digits, `.`, `_`, `+`, and `-` |
| `residency.approved` | boolean `true`. Any other value is `config.residency` |
| `sample_policy` | `none` |
| `temperature` | JSON lexeme `0` |
| `output` | `json_schema` |
| `trust.ca_file` | filesystem path of a PEM certificate bundle. The file is at most 65536 bytes |
| `secret.bearer_file` | filesystem path of the bearer token. The file is at most 512 bytes |
| `budget` | every field below |
| `declared_price.currency` | `USD` |
| `declared_price.per_total_token` | non-negative decimal with at most 8 fractional digits, no exponent |

Unknown keys are rejected. `billing_certificate` is not a config field.

The joined URL is `origin` plus `path`. Its scheme, host, port, and path must still be the approved values. The model id must equal the prepared request `model_id`. The caller names this builder endpoint. It is not read from an agent endpoint, a proxy variable, or `OPENAI_BASE_URL`.

### Budget

A number outside the range is `config.budget`.

| Field | Range |
|---|---|
| `max_tables` | 1 to 16 |
| `max_request_bytes` | 1 to 65536 |
| `max_response_bytes` | 1 to 65536 |
| `max_response_chunks` | 1 to 64 |
| `deadline_ms` | 1 to 30000 |
| `max_prompt_tokens` | 1 to 100000 |
| `max_completion_tokens` | 1 to 100000 |
| `max_total_tokens` | 1 to 200000 |

`deadline_ms` bounds the HTTPS call from the start of the connection attempt through the last accepted body chunk. It does not bound the local compiler.

### Secret file

The bearer is the file bytes without one optional trailing newline. It is 16 to 256 bytes of visible ASCII (`0x21` through `0x7e`) and contains no whitespace. A bad file is `config.secret`. The token is placed only in the `Authorization` header. It is not written into the chat body, the prompt, argv, the outcome, or an error line. If the config text, the command, or the prepared request already contains those bytes, the call is `config.secret` and no socket is opened.

The CA file is a certificate bundle. It is not a private key. `Certificate::from_pem_bundle` must return at least one certificate. The client trusts those certificates and no platform roots.

## Client

reqwest `0.13.5` with `default-features = false` and features `rustls` and `http2`, the pin already used by `oo-sdk`. The call uses the 0.13.5 builder methods `redirect(Policy::none)`, `retry(retry::never())`, `no_proxy`, `https_only(true)`, `http1_only`, `tls_certs_only`, `referer(false)`, and `connection_verbose(false)`. The body is read with `Response::chunk` under `max_response_chunks` and `max_response_bytes`. `http1_only` keeps that reader on one HTTP/1.1 response. The `http2` feature is the reviewed pin. This call does not open an unbounded stream.

The client does not read `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `OPENAI_API_KEY`, or `OPENAI_BASE_URL`. There is one attempt and one URL. A redirect is `provider_failure` / `redirect` and is not followed. A wrong CA or a failed handshake is `provider_failure` / `tls`. A deadline expiry is `provider_failure` / `timeout`.

## Chat request

The body is one JSON object:

| Field | Value |
|---|---|
| `model` | the prepared `model_id` |
| `temperature` | `0` |
| `stream` | `false` |
| `max_completion_tokens` | the config budget |
| `messages[0]` | `system`, content equal to `boundary.instructions` |
| `messages[1]` | `user`, content equal to the prepared request document |
| `response_format.type` | `json_schema` |
| `response_format.json_schema.name` | `oo_builder_description_response_v1` |
| `response_format.json_schema.strict` | `true` |
| `response_format.json_schema.schema` | object schema for `oo.builder.description-response/v1` |

`boundary.instructions` must be the description-candidate prompt and nothing appended. `statements.samples` must be `not_included`. The substrings `sample_values` and `samples_masked` are absent. A miss is `provider_failure` / `samples` with no socket. A prepared `model_id` that differs from the config model is `provider_failure` / `model` with no socket. A prepared table count above `max_tables`, or a chat body above `max_request_bytes`, is `budget_failure` with no socket. Each of these still calls `complete_description_candidates`, so the structural draft is kept and no candidate is emitted.

The schema asks for `protocol`, `request_sha256`, `schema_hash`, `evidence_hash`, `prompt_version`, `prompt_sha256`, `model_id`, `source_id`, `budget_sha256`, `complete`, and `candidates` of `dataset`, `card_sha256`, `label`, `description`, `grain`, `caveats`, `open_questions`, and `confidence`. The compiler, not this schema, admits candidates.

## Provider response

The raw body, any decoded assistant content, and response header values are scanned for the bearer bytes before any candidate is admitted. A hit is `provider_failure` / `secret_echo`. The body is not copied into the outcome and is not logged. A chunk that would exceed `max_response_bytes` or `max_response_chunks` is scanned and then dropped. It is not stored.

Status mapping, after a body that stayed inside the byte and chunk caps:

| Observation | `captured_response.status` | Transport refusal or reason |
|---|---|---|
| HTTP 200, one choice, `finish_reason=stop`, assistant content, usage present, prompt plus completion equals total, usage inside the token caps, content free of the bearer | `complete` | none |
| HTTP 200, `finish_reason=length` | `truncated` | none |
| HTTP 200, `finish_reason` null, or `stop` with no content, or no choices | `incomplete` | none |
| HTTP 200, `stop`, usage above a token cap | `budget_failure` | `tokens` |
| body or chunk cap exceeded while reading | `budget_failure` | `response_bytes` or `response_chunks` |
| HTTP 200, usage arithmetic does not match, or the envelope is not one completion object | `provider_failure` | `malformed_envelope` |
| HTTP 400 through 599 | `provider_failure` | `http_status` |
| connection closed after some body bytes and before the cap | `incomplete` | `partial` |
| deadline | `provider_failure` | `timeout` |
| redirect | `provider_failure` | `redirect` |
| handshake or connect failure | `provider_failure` | `tls` |
| bearer bytes in the body, decoded content, or headers | `provider_failure` | `secret_echo` |
| prepared request is not sample policy none | `provider_failure` | `samples` |
| prepared `model_id` differs from the config model | `provider_failure` | `model` |

Extra completion fields are ignored. More than one choice is `malformed_envelope`. Non-200 bodies are not parsed for usage. A `stop` that fails the envelope still uses `provider_failure`, not `complete`.

`complete` copies the assistant content into `captured_response.body` with `byte_len` equal to that string's UTF-8 length. Every other status omits `body` and `byte_len`. The compiler then ignores any content that would have parsed.

## Outcome

Exit 0 writes one JSON object and no newline. Exit 1 writes nothing to stdout. Stderr is `description-provider: <code> <message>`. Exit 2 is usage. A compiler `Err` is exit 1 with that compiler code and static message. The path is not printed.

| Field | Value |
|---|---|
| `protocol` | `oo.builder.description-provider-outcome/v1` |
| `compiler` | `oo-builder/description-provider/v1` |
| `transport.status` | `complete`, `truncated`, `incomplete`, `provider_failure`, or `budget_failure` |
| `transport.http_status` | number, or null when no response status was read |
| `transport.finish_reason` | string, or null |
| `transport.usage` | `{prompt_tokens, completion_tokens, total_tokens}`, or null |
| `transport.declared_cost` | null, or `{currency, per_total_token, amount, billing_certificate:false}` |
| `transport.budget_reason` | null, `tables`, `request_bytes`, `response_bytes`, `response_chunks`, or `tokens` |
| `transport.refusal` | null or one refusal name |
| `transport.caller_residency_approval` | `explicit` |
| `transport.residency_certificate` | `false` |
| `transport.billing_certificate` | `false` |
| `transport.sample_policy` | `none` |
| `candidate` | the document returned by the description-candidate API |

`amount` is `total_tokens` times `per_total_token`, exact decimal, same fractional scale. It is a declared price report. `billing_certificate` stays false. Usage is reported when a 200 body had consistent usage, including token `budget_failure` and `secret_echo`. It is omitted when the meter is absent or inconsistent.

When `prepare_description_request` already returns `oo.builder.description-candidate/v1`, that document is `candidate` and no socket is opened. `transport.status` and `transport.budget_reason` copy that document.

When prepare returns a request, this unit calls `complete_description_candidates` with the original command plus `captured_response`. The compiler keeps `structural_draft` and returns no candidates for `truncated`, `incomplete`, `provider_failure`, `budget_failure`, and `format_rejected`. A `budget_failure` status is recorded by that compiler as `budget_reason=caller`. The precise reason stays on `transport.budget_reason`. Compiler statements are not rewritten: `llm_transport` stays `not_used`, `residency` stays `not_approved`, and `cost` stays `not_metered`.

## Out of scope

No cache, retry, second endpoint, sample other than `none`, customer row, raw comment body, column-level candidate, real provider key, production network call, invoice, residency certificate, or edit to the description-candidate compiler. Full S5 is not accepted here.
