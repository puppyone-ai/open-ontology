# Local LLM loopback relay

Contract `oo.local-llm-relay/v1`. Binaries `oo-local-llm-relay` and `oo-local-llm-models`.

This crate is a separate process. It does not modify `tools/ontology-dialogue` or `tools/description-provider`. Those programs keep `https_only`, a caller-supplied CA, no proxy, no redirect, and no retry. This relay is the only place the authorized cleartext HTTP backend is dialed.

The human authorization for this demo is the local OpenAI-compatible endpoint `http://127.0.0.1:8317/v1`. The backend host and port are constants. There is no flag, environment variable, or request field that changes them.

## Fixed archive seam

Fixed commit `e8d297f95b32052ea587fe8ad67b6e8493a00300` contains `tools/description-provider` and does not contain `tools/ontology-dialogue`. Dialogue remains owned by a300. a300 proof-7 built the dialogue tests, then `tests/live.rs` panicked with the static message `execute binary` (exit 101) and did not publish `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, or a compiled dialogue binary. This crate does not compile dialogue from the moving workspace and does not spawn `query-pg`. A models call through this relay is not an ontology-dialogue proposal, approval, or execution. Model SQL is never executed here.

When a fixed dialogue binary exists, the unchanged `propose` config points `endpoint.origin` at this relay, `endpoint.path` at `/v1/chat/completions`, `trust.ca_file` at this relay's public certificate, and `secret.bearer_file` at the private token file. `execute` remains dialogue's checked handoff to `query-pg`. This crate does not perform that handoff.

## Relay

```text
oo-local-llm-relay --listen 127.0.0.1:<port> --cert <cert.pem> --key <key.pem>
```

`--listen` is exactly `127.0.0.1` and a port from 1 to 65535. `0.0.0.0`, hostnames, and other addresses are `config`. One listening socket. The certificate file is a regular non-symlink PEM of at most 65536 bytes, contains `BEGIN CERTIFICATE`, and does not contain `PRIVATE KEY`. The key file is a regular non-symlink PEM of at most 65536 bytes and stays on the host that binds the socket. It is not an argument value, not an environment variable, and not written to evidence or Git.

Rust 1.94 makes `std::env::remove_var` unsafe, and this crate forbids unsafe. The process therefore does not clear the environment. It refuses with `config`, before it builds a client, when any of `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`, `OPENAI_API_KEY`, `OPENAI_BASE_URL`, or `OPENAI_API_BASE` is set, including the lowercase proxy names. It does not read those values into a log. Launch scripts unset the names before starting the process.

TLS is rustls with TLS 1.2 or newer and ALPN `http/1.1` only. No client-certificate request. `--cert` is an end-entity certificate, not a CA. rustls rejects a CA certificate presented as the server certificate. Callers trust the issuing CA through their own CA file. The end-entity SAN contains `IP:127.0.0.1` and, for a Docker client, `DNS:host.docker.internal`.

### Allowlist

| Method | Path |
|---|---|
| `GET` | `/v1/models` |
| `POST` | `/v1/chat/completions` |

The path is origin-form, exact, and has no query, fragment, scheme, or dot segments. Any other method or path is HTTP 404 and does not dial. `CONNECT` is 404 and does not dial.

`POST` requires one `Content-Type` whose value begins with `application/json` and one `Content-Length`. `GET` rejects a non-zero content length. `Transfer-Encoding` is rejected. One `Authorization` header is optional; its value is 1 to 600 bytes, has no CR or LF, and is forwarded only as that header. The value is marked sensitive and is not written to stdout, stderr, or a file by this process.

### Upstream

One HTTP/1.1 request to `http://127.0.0.1:8317` plus the allowlisted path. The client is reqwest `0.13.5` with `redirect` none, `retry` never, `no_proxy`, `http1_only`, idle pool size 0, and no `https_only` (the authorized backend is cleartext HTTP). A 3xx response is not followed and becomes HTTP 502 to the caller with an empty JSON object `{"error":"upstream"}`. Connection failure, a response body over the cap, or a chunk count over the cap is the same 502. There is no second attempt.

The `Host` header sent upstream is `127.0.0.1:8317`. The caller's `Host` is not used.

### Caps

| Cap | Value | Over-cap result |
|---|---|---|
| Request headers | 8192 bytes | close, no dial |
| Request body | 65536 bytes | HTTP 413, no dial |
| Response body | 65536 bytes | HTTP 502, body dropped |
| Response chunks | 32 | HTTP 502, body dropped |
| In-flight connections | 2 | HTTP 503, no dial |
| Deadline | 30000 ms from accept | close or HTTP 504 |

Dialogue's own response cap of 16384 bytes still applies inside dialogue. This relay does not raise that client cap.

Stderr is `oo-local-llm-relay: ` plus one static token: `config`, `bind`, `tls`, `io`. Request and response bytes are not part of the token.

## Models client

```text
oo-local-llm-models --origin <https-origin> --ca <cert.pem> [--bearer-file <path>] --out <path> [--chat-out <path>]
```

The origin uses the dialogue rule: `https`, a host, no user, password, query, fragment, or path other than `/`. The same environment refusal as the relay applies before the client is built. The client is reqwest with `https_only`, the given CA bundle and no platform roots, `redirect` none, `retry` never, `no_proxy`, and `http1_only`. It dials `/v1/models` only, then optionally one `/v1/chat/completions` when `--chat-out` is set and a grammar-valid model id was selected.

The bearer file is optional. When it is set, the client reads it itself. The bytes are not an argument and not an environment variable. One trailing LF is stripped. The remaining bytes must be 1 to 512 visible ASCII bytes (`0x21` through `0x7e`) or the client exits `config` without a socket. Dialogue's own rule remains 16 to 256 bytes. This client still makes one request when the length is outside that range, and records `dialogue_bearer` as `below_minimum`, `ok`, or `above_maximum`. The length is recorded. The bytes are not.

The output file is one JSON object. It records the HTTP status, the selected model id, the candidate count, and the grammar-match count. It does not contain the provider body. A model id is selected only when it matches dialogue's `model_id` rule: 1 to 128 bytes of ASCII letters, digits, `.`, `_`, `+`, or `-`. The first matching `data[].id` in array order is selected. Ids that fail the rule are counted and not stored. If the bearer occurs in the output, the client refuses to write it and exits `secret`.

`--chat-out` sends one non-streaming chat body with `temperature` 0, the dialogue system sentence, and a fixed public synthetic question. It does not send a Pack, a database row, or the bearer inside the JSON body. The chat output records the HTTP status and whether the body was one `oo.ontology-dialogue.sql-candidate/v1` object. It does not record the SQL text, the provider body, or a proposal hash. Nothing in this client executes SQL.

## Docker and the host

Pre-edit probe on the pinned image: a container's `127.0.0.1:8317` is closed, and `host.docker.internal:8317` is open, while the provider listens on the host's `127.0.0.1:8317`. The relay process therefore runs on the host and dials `127.0.0.1:8317`. Its private key is not mounted into a container. A Docker client reaches the host loopback listener through `host.docker.internal` and trusts the public CA. The end-entity certificate's SAN contains `IP:127.0.0.1` and `DNS:host.docker.internal`. No host network, no Docker socket, and no published privileged port.

## Pins

Direct versions match the root lock and the description-provider manifest: reqwest `0.13.5`, rustls `0.23.45`, tokio `1.53.1`, tokio-rustls `0.26.5`, httparse `1.10.1`, serde `1.0.229`, serde_json `1.0.151`, sha2 `0.10.9`, base64 `0.23.1`, plus the same non-API pins description-provider keeps for reqwest (`hyper-util` `0.1.20`, `rustls-platform-verifier` `0.7.0`, `cc` `1.4.7`, `find-msvc-tools` `0.1.13`, `yoke-derive` `0.8.2`). Transitive crates that resolved outside the fixed archive are also direct exact pins (`crypto-common` `0.1.6`, `generic-array` `0.14.9`, `h2` `0.4.19`, `hyper` `1.11.1`, `js-sys` `0.3.105`, `libc` `0.2.189`, `mio` `1.2.3`, `quinn-proto` `0.11.18`, `quinn-udp` `0.5.15`, `smallvec` `1.16.1`, `want` `0.3.1`, `wasm-bindgen` `0.2.128`, `wasm-bindgen-futures` `0.4.78`, `wasm-bindgen-shared` `0.2.128`, `web-sys` `0.3.105`, `zeroize` `1.9.0`). They are not relay APIs. `wasm-bindgen-futures` `0.4.79` requires `js-sys` `=0.3.106`, so that pair is pinned together to the archive versions. This crate is its own workspace. It is not a member of the root workspace and does not edit the root lock.
