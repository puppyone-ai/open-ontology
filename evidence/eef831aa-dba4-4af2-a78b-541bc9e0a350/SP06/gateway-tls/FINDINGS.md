# FINDINGS

Raw logs stay in `linux/proof` through `linux/proof-6`. Nothing below rewrites those exits.

## Failed attempts

| attempt | docker-run.exit | container phase | what the log shows |
| --- | --- | --- | --- |
| `proof` | 101 | `clippy_exit=101` | library clippy: unread `TlsPeer.remote`; `result_large_err` because `gate` and `verified_peer` returned `Response` |
| `proof-2` | 101 | `clippy_exit=101` | `http_gateway_tls.rs` `expect_err` needs `Debug` on `OperatorLoad`. Library emitted no diagnostic. No test ran |
| `proof-3` | 101 | `clippy_exit=101` | `useless_format` and `single_match` in the TLS test. Library emitted no diagnostic. No test ran |
| `proof-4` | 101 | `tls_exit=101` | clippy 0; existing loopback suite 8 passed. New tests failed at runtime. `plain_exchange` half-closed before reading and panicked at `header terminator` with an empty parse. `harness_tls_exchange` returned `read` after the handshake string was not the error |
| `proof-5` | 101 | `clippy_exit=101` | `E0308` `read_to_end` is `Result<usize, _>` but the arm was `Ok(())`. `E0658` `slice::split_once` is unstable. No test ran |

`proof-4` is the real empty plain read and the real TLS `read` failure. The later client change does not treat that log as a pass.

## Transport limit

`harness_tls_exchange` still returns `handshake`, `connect`, `write`, `ca`, or `client_cert` for those failures. An anonymous client and a wrong CA stay errors. The read path returns `read` when the buffer is short of a complete message.

If `read_to_end` returns an error and the buffer already contains the header terminator plus every byte declared by `Content-Length`, those existing bytes are returned. No byte is added to satisfy the length. A missing `Content-Length`, a truncated body, or a buffer without `\r\n\r\n` stays `read`. This is the abrupt-close limit: the gateway closes the TCP socket after `Connection: close` and this client does not require a TLS `close_notify` once the declared HTTP message is already present. Proof-6 does not record which branch handled each response. It records that the HTTPS responses parsed as HTTP. Proof-4 records that the same call returned `read` before this retention existed.

The plain loopback helper no longer shuts the write half down. It writes the request and reads until EOF, which is the helper already used by the passing `http_gateway_loopback` suite. It does not accept a buffer that lacks a header terminator.

## What proof-6 does not show

- E48, SP06, public identity, OAuth, IdP, token issuance, or session support are not accepted.
- `/internal` and `oost_` remain refused. A workload URI is not a role. The proof does not add an authorized issuer.
- `POST /v1/sql` stays `501` in the unit test at the body limit. Untrusted SQL was not added. No audit claim and no owner grant.
- Clippy did not use `--all-targets`. Other `oo-gateway` tests were not run.
- No Cargo.toml, Cargo.lock, or new registry dependency changed. The hyper feature-edge amendment was not made.
- The pre-edit smoke was not rerun.
- Sibling evidence was only read. a328 focused-10 had been `docker-run.exit` 0 when last read; its separate smoke exit was 1. a351 `test.exit` is 0 (6 tests in `test.log`), and types, lint, contrast, build, bundle, and storybook are 0. `test-attempt-3.exit` is 1, `playwright-install-deps.exit` is 1, and `browser-libs.exit` is 100. No a351 file was written and no browser was started here.
