# Findings

## F1. The first review Clippy failure is an oracle defect

`linux/smoke/review-clippy.stderr` reports one error: `clippy::useless_format` in the overlaid review test at line 146. `cargo clippy --keep-going` printed no other diagnostic. The archive author Clippy, loopback, TLS default, and the author private-PostgreSQL live test had already exited 0.

`tools/review_tls.rs` now uses a byte-string request. Smoke-2 Clippy on `http_gateway_tls_review` exited 0. Product sources were not edited. `linux/smoke-2/work-sha256-after.txt` matches `linux/smoke/work-sha256.txt`.

The same edit makes the handshake probe fail unless the stalled TCP connection closes. After a single `0x16` byte and 1200 ms, with `handshake_timeout_ms` 400, the read returned a close within 400 ms, and a later leaf `/health` was 200. `review-summary.txt` records `handshake_timeout=dropped_then_ok`.

## F2. The abrupt-close helper keeps a complete Content-Length body

`harness_tls_exchange` returns the buffer when `read_to_end` errors and `http_message_complete` is true. That predicate requires the header terminator and `body.len() >= Content-Length`. Smoke-2 observed:

- declared length 4 with body `okay`: accepted
- declared length 8 with body `short`: `read`
- no Content-Length: `read`
- no `\r\n\r\n`: `read`
- declared length 4 with body `okayTAIL`: accepted
- declared length 0 with an empty body: accepted

Bytes past the declared length are not a truncated body. The gateway still closes the TCP socket without a TLS `close_notify`. This matches the helper in the reviewed commit and the a350 finding on that helper.

## F3. Clippy was absent in each fresh container and was installed there

The pinned Rust image does not keep a Clippy install across containers. `linux/smoke/clippy-component.stderr` is `info: downloading component clippy` and `clippy-component.exit` is 0. That install did not persist on the cargo volume.

Smoke-2 `clippy-component.list` contains `cargo-aarch64-unknown-linux-gnu`, `rust-std-aarch64-unknown-linux-gnu`, and `rustc-aarch64-unknown-linux-gnu`. The phase is `clippy_component_absent_install`. `clippy-component.stderr` is again `info: downloading component clippy`, and `clippy-component.exit` is 0. The component was installed inside this review's builder. It was not installed on the host. Host `cargo` / `rustc` / `rustdoc` / `rustup` stayed 0.

## F4. No product clause failed

The frozen E48-D1/1 clauses exercised here held: non-loopback TLS config loads, a loopback config on `0.0.0.0` is `listen`, omitted transport still loads, bad TLS material exits 1 with `oo-gateway: tls_key` and no `listen.addr`, argv is the directory only, identity is the verified leaf URI, forged certificate and workload headers are 403 and do not echo the marker, anonymous, other-CA, and DNS-only leaves get no HTTP 200, plaintext on the TLS port is not HTTP 200, the two compiled roles keep their own columns and digests, a second certificate changes `peer_workload` and leaves the token role, wrong audience / expired / revoked / missing bearer are 401, an `oost_` token is 401, `POST /internal/v1/session-tokens` is 403, and `POST /v1/sql` is 501 at the body limit and 413 one byte over. The 5000 ms positive cache stayed warm at 1s and observed revocation and a closed database after 5.2s.

No production credential, customer call, or private key is in this packet.
