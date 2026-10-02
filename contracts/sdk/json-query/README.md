# SDK JSON query

Production Rust client contract for one call: `POST /v1/query` with a JSON `QueryRequest` and a JSON `QueryResponse` or error envelope. The implementation is `oo-sdk`. It uses the frozen `oo-core` API door (`parse_api_json`) before send and again on the response. `reqwest`'s JSON helper is not used.

This directory is the contract and the synthetic fixtures for that client. The loopback HTTP and TLS servers in `crates/oo-sdk/tests` are a protocol oracle. They are not a Gateway, not PostgreSQL, and not the SP09 end-to-end test.

## Request

The caller passes a base origin and an optional mount prefix. The client appends `/v1/query`. A base that already ends in `/v1/query`, contains userinfo, a query string, a fragment, `%`, `\`, `@`, or a CR/LF byte, or uses an empty, `.`, or `..` path segment, is rejected before a socket is opened.

`https` is the production scheme. `http` is accepted only when the caller selects local development and the host is `localhost`, `127.0.0.0/8`, or `::1`. TLS certificate verification is never disabled. There is no API that turns it off.

The bearer token is caller-supplied bytes. The client does not read a token from the environment or a file. `Debug` and `Display` render `BearerToken([redacted])`. The value is sent only as the `Authorization` header on the parsed origin. This slice does not wipe process memory on drop.

The wire body is canonical JSON: serde_json object keys in sorted order, no extra whitespace. Optional fields that would have been JSON null are omitted. The bytes are accepted only when `parse_api_json::<QueryRequest>` accepts them and the typed value is unchanged. A body over the caller cap, or over the public 1 MiB HTTP ceiling, fails before connect. The client does not truncate the SQL or the body to force it under the cap.

Public ceilings, enforced by `oo-core` on the typed values and by this client on the HTTP buffer, are: HTTP body 1 MiB, SQL 64 KiB, `max_rows` 1..=2000, `timeout_ms` 1..=120000, cell text 500 characters. A response whose `returned_rows` does not match the row array, or whose row width does not match the column list, is a protocol failure.

## Response

The client reads the body incrementally and stops at the response cap. It does not allocate from `Content-Length` and does not treat a missing or dishonest length as permission to buffer without a cap. A declared length above the cap fails before the body is parsed. Bytes past the cap are not parsed. An incomplete read, reset, or timeout is a transport error; a partial buffer is not parsed as success.

Success requires HTTP 200 and `Content-Type: application/json`, optionally with `charset=utf-8` and no other parameters. Any other status is an error envelope only when the body parses, `ErrorEnvelope::consistent` holds, and the code's HTTP status matches the response status. An HTML page, a 200 that contains an error envelope, and a status that disagrees with the error code are protocol failures.

Gateway errors keep `ErrorCode`, the HTTP status, `retryable`, `retry_after` when the `Retry-After` header is one decimal-second value, and `query_id` when the envelope has one. `Debug` and `Display` omit the token, the SQL, the URL, the body, and the message text. Accessors return message and hint only after those secrets are stripped. Stripping is not a memory wipe.

A client timeout or a dropped connection means this process stopped waiting. It is not a confirmation that a database backend cancelled work. This client has no cancellation API. `query_cancellation` is listed in `oo_sdk::UNSUPPORTED` with export, Arrow, discover, describe, resolve, search, freshness loading, sandbox environment loading, and an OpenAPI client.

## HTTP handling

Redirects are not followed. Proxy environment variables are not read; the client calls `no_proxy`. The token is not attached to a redirect target because the redirect is not requested. TLS uses rustls with the process trust store unless the caller passes `RootTrust::OnlyPem`. That mode uses only those PEM bytes as roots and still checks the hostname. The client does not read a CA file or an environment variable. An empty bundle is rejected. There is no result cache and no process-global identity.

## Retry

Automatic retry is a narrow subset, not the whole SP09 retry matrix. The default allows 3 extra attempts and waits at most 2 seconds. `RetryPolicy::none` disables it. A retry happens only after a complete `RATE_LIMITED` (429) or `OVERLOADED` (503) envelope whose status matches `ErrorCode::http_status` and whose `retryable` flag is true. The wait is the server's decimal `Retry-After` when that value is within the configured maximum. A larger `Retry-After` is not shortened and is not retried. A missing, duplicated, or non-decimal `Retry-After` uses 200 ms, 400 ms, then 800 ms, capped by the same maximum and by the overall deadline.

Authentication failures, `TIMEOUT`, other non-retryable gateway codes, `INTERNAL`, `AUDIT_UNAVAILABLE`, transport failures, and protocol failures are not replayed. `INTERNAL` and `AUDIT_UNAVAILABLE` still surface `retryable: true` from the core code table so the caller can decide. This client does not replay them.

## Fixtures

`fixtures/query-request.json` and `fixtures/query-response.json` are the legal API documents this client must accept. `fixtures/query-request.wire.json` is the exact POST body. `fixtures/query-response-empty.json` and `fixtures/query-response-truncated.json` are legal empty and truncated results. The `error-*.json` files are gateway envelopes. The `reject-*.json` files must not parse as success.

`fixtures/tls/` holds a synthetic CA, a `localhost` certificate, and a second CA that must not verify that certificate. The private keys exist only so the oracle can complete a handshake. They are not production trust anchors. Tests must not print them.

## Out of scope

Full SP09-W01 acceptance, AT-API-01, OpenAPI publication, query cancellation, CLI conversation, real PostgreSQL end-to-end, and supply-chain admission are not decided by this contract. New dependency obligations are recorded in the task evidence and are not claimed as reviewed exceptions.
