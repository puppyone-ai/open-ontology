# a089 SDK error redaction and retry correction

Delivery commit `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df`, tree `db0717173187604182b02d4fc8227b107bddb6d1`, parent `6831d6e862303a0ce1c883925068078cbcb78e23`.

Linux arm64, Rust 1.94.0, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, bridge network, `--cap-drop ALL`, `no-new-privileges`, `--init`, no socket, no HOME mount, `HOME=/tmp`, memory 8g, cpus 4, pids-limit 4096. One owned builder at a time. Host `cargo` and `rustc` were not running. Fetch used the network. This is not an air-gap build.

Compiler on the final run: rustc 1.94.0 (`4a4ef493e` 2026-03-02), host `aarch64-unknown-linux-gnu`, cargo 1.94.0 (`85eff7c80` 2026-01-15).

## Phase exits

Baseline on the unfixed archive `77d957750e18c7e811e845a7717d54372b41839e`, before these edits. Logs in `linux-baseline/`.

| Phase | Exit |
| --- | --- |
| rustc | 0 |
| cargo | 0 |
| clippy component | 0 |
| fetch | 0 |
| test | 0 |
| clippy | 0 |
| docker | 0 |

Unfixed archive plus the new tests only. `red/test-lib.exit` is 101. `red/test-integrations.exit` is 101. `red/docker-integrations.exit` is 101. Existing `http_query` 18 passed, `proxy_env` 1 passed, the original TLS test passed. The new assertions failed. Masked lines are in `red/masked-failures.txt`. No token or SQL text is in those logs.

Final overlay: the same archive plus the owned SDK sources, tests, and `contracts/sdk/json-query/README.md`. Lock and manifests match the archive (`cmp`). Logs in `final/`.

| Phase | Exit |
| --- | --- |
| rustc | 0 |
| cargo | 0 |
| clippy component | 0 |
| fetch | 0 |
| test | 0 |
| clippy | 0 |
| docker | 0 |

Final inspect: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`. Fetch command was `cargo fetch --manifest-path Cargo.toml --locked` with no `-p`. Test and clippy were `--locked --offline -p oo-sdk`. Clippy used `--all-targets -- -D warnings`.

Final test counts: lib 6, `http_query` 18, `proxy_env` 1, `redaction_retry` 9, `tls_query` 2, doctests 0. All passed.

Owned file hashes:

| File | SHA-256 |
| --- | --- |
| `crates/oo-sdk/src/wire.rs` | `8f3323cf6e542ce2f46759ab7ffa85d4ae69e24e7c692aa7c27c495e1507775d` |
| `crates/oo-sdk/src/token.rs` | `e4e46321217eab1ce41411269a1e5dea0897afb9d241d4a712148c877f2ffb28` |
| `crates/oo-sdk/src/client.rs` | `6c16b2018cb6b52c04f55e08380edff585b092e983839e73ce9e03e556f6ef2b` |
| `crates/oo-sdk/src/error.rs` | `74f46ac025b1adb768928f0429a3e6eddd0d35f3e882325f360b752117e28821` |
| `crates/oo-sdk/tests/redaction_retry.rs` | `af7fde557b64b49fd649ee7d9437cd1f21bad8d0434e27b163b27d2b9a6325c4` |
| `crates/oo-sdk/tests/tls_query.rs` | `8227f519ec157d341a314ebacb0df2cd1b1c69b98ad49cbe453704ea45a08ea9` |
| `contracts/sdk/json-query/README.md` | `1143d43377cad5300ee8dacf040023d8f84df73cd3ddb35aabb7f8cf13301b84` |

## Findings

1. `SDK-SCRUB-SHORT-TOKEN`. `scrub_text` no longer requires a token of 8 bytes. Every bearer `BearerToken::new` accepts is removed from `message()` and `hint()`. Lengths exercised: 1, 6, 7, and the old boundary 8. The length-1 token is still accepted. Standard fixtures stay `slow down` / `retry later` and `parse` / `fix` when those strings do not contain the token. Before the fix, lengths 1, 6, and 7 remained (`message`/`hint`/`Bearer` lengths in `red/masked-failures.txt`). Length 8 was already removed. Debug and Display stayed redacted.

2. `SDK-SCRUB-SHORT-SQL`. Every nonempty request SQL is a secret, including a quoted literal, a token that is a prefix of the SQL, a token inside the SQL, a newline, and Unicode. `+` is treated as a space. Empty SQL is not a needle, so static error text stays. The POST body still contains the SQL. The request door is unchanged. Before the fix the short SQL lengths 19, 8, 4, 22, 66, and 8 remained in the message.

3. `SDK-SCRUB-PERCENT-TOKEN`. A secret byte may appear raw, as one `%HH` of either hex case, as `\/` for `/`, or as `+` for space. The gateway text is not percent-decoded, so `%2541bcdEF12` is left in place for token bytes that are not that encoding. Work stops at 4_000_000 comparisons. Output stops at 16 KiB. A field that would still contain a secret becomes a static omission (`[redacted]`, `[omitted]`, or `****`, or empty when every marker contains a secret). Before the fix, encoded lengths 12, 24, 24, and 33 remained. The length-12 form is the `Abcd%45%4612` shape. A `Z` byte covers mixed hex case (`%5A` / `%5a`). Assertions record lengths only.

4. `SDK-RETRY-AFTER-9-DIGITS`. `Retry-After` is `Backoff`, `Decimal`, or `Overflow`. Missing, duplicate, and non-decimal headers use 200 ms, then 400 ms, then 800 ms. That hold passed on the unfixed client (`declared_backoff_and_retry_bounds_stay`). Leading zeros are numeric. All zeros inside the 4096-byte scan are a zero wait. A decimal greater than or equal to `max_wait`, or an all-digit value that does not fit the scan or `u64`, does not retry and is not shortened. `retry_after()` is `Some` only for a decimal value. Before the fix, 9-digit `100000000` retried (`hits=2`, `elapsed_ms=244`, `after_ms=none`), as did leading-zero `000000003` (`243`), equal-to-max `2` (`hits=2`, `elapsed_ms=2057`), overloaded 9-digit (`252`), 30 nines (`245`), 21 nines (`243`), `2^64` (`256`), and 4097 zeros (`264`). Nine, 128, and 4096 zeros slept about 230–260 ms instead of retrying immediately. After the fix the same suite passed in 1.22 s, inside the elapsed bounds (immediate zero-wait under 140 ms, backoff between 150 ms and 700 ms, no-retry under 500 ms, wall-budget under 350 ms). Retry is still only a complete 429 `RATE_LIMITED` or 503 `OVERLOADED`. The 401 hold stayed at one hit. Three extra attempts with `Retry-After: 0` stayed at 4 hits.

5. `SDK-SCRUB-PATH`. The scrub removes the serialized query URL, its origin, authority, mount path, and `/v1/query` path. Scheme bytes fold. The longest match is kept, so an origin cut does not leave the mount path. `Client` debug no longer prints the URL. `QueryError::source` is `None`. Before the fix the message still held locator lengths 15, 6, 42, 111, 37, and 15, and client debug held lengths 7 (`http://`), 6 (`/mount`), and 9 (`/v1/query`).

6. `SDK-HEADER-SENSITIVE`. `BearerToken::new` calls `HeaderValue::set_sensitive(true)`. The unit test checks `is_sensitive`, debug text `Sensitive`, and that a `HeaderMap` debug does not contain the token. Before the fix the flag was 0 and the header debug length was 10. The wire header is still `Bearer` plus the token. That check compares lengths and bytes and does not print them.

A 2000-byte run of a one-byte token is omitted as a whole field. The hint `retry later` stays when it does not contain that token. A one-letter token `a` cannot use the marker `[redacted]`, because that marker contains `a`.

## Out of scope

Supply findings above. Secret zeroization. Real Gateway, PostgreSQL, exports, OpenAPI, discover, CLI, amd64, and release.
