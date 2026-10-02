# a090 — SDK runtime correction, fixed review

**Decision: approve**, scoped to the six runtime findings. Full SDK, supply admission, Gateway, CLI, PostgreSQL, and amd64 stay unsigned.

The independent oracle on the delivery archive passed 55 required cases. Developer smoke also passed. Smoke exit 0 is the package test suite. The oracle is the acceptance evidence for the six findings.

## Identity

| | |
| --- | --- |
| Delivery | `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df` |
| Tree | `db0717173187604182b02d4fc8227b107bddb6d1` |
| Parent | `6831d6e862303a0ce1c883925068078cbcb78e23` (`parser: fail closed on contradictory IPC and bound worker cleanup`) |
| Subject | `sdk: scrub accepted secrets and stop overlong Retry-After` |
| Author date | 2026-10-02 11:07:40 +0800 |
| Stamp | `fc877c482a61729ea46e779a674a46f8f72c80c5`, parent is the delivery commit, subject `sdk: record the a089 delivery commit`. Name-status is `HANDOFF.md` and `REPORT.md` under a089 only. |
| Archive sha256 | `a2914f28ff624a4fdc29eadb38e770e7930eb2433aa5dbb66fe2068f4f7e9631` |

Acceptance is `git archive` of the delivery commit. The workspace lock was dirty and was not used (`linux-smoke/input-refs.txt`, `worktree_lock_not_used=yes`). Lock blob `4aea90f3f6d4822c680c4dcee9985996439ee33c`, manifest blob `c78ee5d0575a11cd6078cf0055cdd2c673edf583`, and `ci/deny.toml` blob `367598cb3768b8c8af698041cd0f84ce6f8fb7be` match the parent. This commit does not change dependencies.

Delivery name-status for product files: `M` README, `client.rs`, `error.rs`, `token.rs`, `wire.rs`, `tls_query.rs`; `A` `redaction_retry.rs` and the a089 evidence tree.

Snapshot sha256: `wire.rs` `8f3323cf6e542ce2f46759ab7ffa85d4ae69e24e7c692aa7c27c495e1507775d`, `client.rs` `6c16b2018cb6b52c04f55e08380edff585b092e983839e73ce9e03e556f6ef2b`, `error.rs` `74f46ac025b1adb768928f0429a3e6eddd0d35f3e882325f360b752117e28821`, `token.rs` `e4e46321217eab1ce41411269a1e5dea0897afb9d241d4a712148c877f2ffb28`. Those match the hashes a089 published for the owned sources.

a084 probe 2 sha256 `f780a4f1871266c1824290ed6eaafd785317c5ec0e6d9198d15b29dafb84fb6e` was rechecked after this review's writes and is unchanged. a078, a084, and a089 were not edited.

## Linux smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. `RUSTUP_TOOLCHAIN=1.94.0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Fetch uses the network. This is not an air-gap claim.

| phase | exit |
| --- | --- |
| `cargo fetch --manifest-path Cargo.toml --locked` | 0 |
| `cargo test --manifest-path Cargo.toml --locked --offline -p oo-sdk` | 0 |
| `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-sdk --all-targets -- -D warnings` | 0 |
| docker run | 0 |

Tests: lib 6, `http_query` 18, `proxy_env` 1, `redaction_retry` 9, `tls_query` 2, doctests 0. Inspect: `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=8589934592`, `PidsLimit=4096`, `HostConfig.Tmpfs` null. Run id `9c6debdc-6167-4bcb-8824-239b51d37201`.

## Oracle

The probe copy adds `a090_oracle.rs`, `a090_probe.rs`, and `#[cfg(test)] mod a090_oracle;` in that copy of `lib.rs`. The snapshot `lib.rs` stayed unmodified. Diff: `probe/probe-tree-diff.txt`. Final source sha256: oracle `d389624fb062626ea4015f009852da6c1c21ee0e6dcb5de2437ae795fe9bf24e`, probe `5d81240c94fd6f9cca57d276d68442f9af668edadc194c55e3784946c556e829`.

Final container `oo-a090-probe-9c6debdc-6167-4bcb-8824-239b51d37201`, `--network none`, `--pull never`, `--cap-drop ALL`, no-new-privileges, memory 8g, cpus 4, pids 4096. Same image and `RUSTUP_TOOLCHAIN=1.94.0`. Reused the smoke cargo and target volumes. `HostConfig.Tmpfs` null. `/src` and `/inside.sh` read-only. Report sha256 `17be939b801c6ccfcfe4882ccabd6c2d24441da2850137e5eb1772abf5157933`.

| attempt | docker | lib | integration | note |
| --- | --- | --- | --- | --- |
| 1 | 101 | 101 | 101 | Harness. Map debug contains the letter `a`. One call passed `Ctx` where a string was required. |
| 2 | 101 | 0 | 101 | `required_failed` is `marker_a`. The hint `retry later` contains `a`. The SDK removed it (`hint_len` 19). The harness had required the original hint. |
| 3 | 0 | 0 | 0 | 56 cases, 55 required, 55 `contract_ok`. Integration finished in 10.47s. |

Commands, both `--locked --offline --test-threads=1`:

- `cargo test --manifest-path Cargo.toml -p oo-sdk --lib a090_`
- `cargo test --manifest-path Cargo.toml -p oo-sdk --test a090_probe`

`rustc -vV` exit 0, same 1.94.0 host triple. Expected socket bytes are handwritten. The report stores lengths, booleans, hit counts, and `retry_after_ms`. It does not store token, SQL, or message text.

## The six findings

`SDK-SCRUB-SHORT-TOKEN`. Lengths 1, 6, 7, and 8: `message_has_token` false, `hint_has_token` false, `auth_exact` true, `body_exact` true, `path_exact` true, hits 1, kind gateway. `message_len` 20 for `slow down ` plus the omission marker. Hint length 11 is the preserved `retry later`. Markers: token `a` yields message length 9 (`[omitted]`), token `e` yields length 4 (`****`), token `*` yields length 10 (`[redacted]`). Combined `e` and `*` yields the empty omission in the direct unit test, because every static marker contains one of those needles.

`SDK-SCRUB-SHORT-SQL`. Lengths 1, 4, 8, and 14, quoted text, `雪`, a newline, a prefix token inside the SQL, and an overlapping token: `message_has_sql` false, hits 1. NUL SQL is rejected locally, hits 0. The duplicate-needle unit test passed.

`SDK-SCRUB-PERCENT-TOKEN`. Partial `%45`/`%46`, uppercase `%HH` of the whole token, and mixed `%5A`/`%5a` are absent. `\/` for a slash SQL is absent. `p+q` for SQL `p q` is absent. A JSON `\u00XX` body decodes to the token and the accessor returns a marker (`message_len` 10).

`SDK-SCRUB-PATH`. `locator_parts` keeps the `slow down ` prefix (`message_len` 64) and the lack checks for the host, `/mount`, and `/v1/query` passed. `locator_scheme_case` message length 20 is the prefix plus one marker. `client_debug_has_locator` is false. Gateway Display contains `q_abc123`.

`SDK-HEADER-SENSITIVE`. The unit test passed. The header is sensitive. Its debug text is `Sensitive`. Map debug is the same for `!`, a long token, and `a`. `BearerToken` Display and Debug are `BearerToken([redacted])`.

`SDK-RETRY-AFTER-9-DIGITS`. Only complete `RATE_LIMITED` (429) and `OVERLOADED` (503) replayed. `INTERNAL`, `AUDIT_UNAVAILABLE`, `TIMEOUT`, and `UNAUTHENTICATED` stayed at hits 1. A 429 body whose code is `INTERNAL`, and `RATE_LIMITED` with `retryable: false`, are protocol failures at hits 1. A truncated body is transport at hits 1, elapsed 8ms. Nine-digit `100000000` is hits 1, `retry_after_ms` `100000000000`. Equal max `2` is hits 1, `retry_after_ms` 2000. `000000001` then success is hits 2, elapsed 1028ms. `000000003` is hits 1, `retry_after_ms` 3000. `u64::MAX` is hits 1, `retry_after_ms` `18446744073709551615000`. The 20-digit overflow and the leading-zero overflow are hits 1, `retry_after_ms` null. All-zero widths 9, 128, and 4096 are hits 4, `retry_after_ms` 0, elapsed 34–36ms. Width 4097 is hits 1, `retry_after_ms` null. Missing, duplicate, `1.5`, and an HTTP-date are hits 4, elapsed 1445–1463ms, `retry_after_ms` null. Three extra attempts on `Retry-After: 0` stop at hits 4, elapsed 32ms. Overall budget 1200ms with `Retry-After: 1` stops at hits 2, elapsed 1023ms, `retry_after_ms` 1000. Zero-wait `RATE_LIMITED` and `OVERLOADED` succeed at hits 2, elapsed 16ms and 13ms.

Direct `scrub_text` also passed the budget case and the output-cap case. The HTTP error door caps message text at 2000 scalars, so those two bounds are not reachable through the envelope. The unit tests call `scrub_text` on the probe copy. Budget exhaustion and the 16KiB cap replace the whole field. The outputs are shorter than 64 bytes and do not contain the unread token. The cap case also lacks a run of `x`.

## Bounds that match the contract

Nested `%2541…` remains (`message_len` 12, `nested_left` true). A literal `\u005a` remains (`message_len` 6, `literal_u_left` true). The raw token is absent in both. One `%HH` pass is the contract. The JSON parser unescapes `\u` and `\/` before scrub, and those decoded forms are removed.

`plus_secret_space_reflection` keeps three characters. `message_has_sql` is false. `+` stands for a space byte in the secret. A space in the text is left alone when the secret itself contains `+`.

A digit string longer than 4096 is `Overflow`. The tail past 4096 is unread. Width 4097 of zeros does not retry and does not use the 200ms backoff.

`127.255.255.255` reached the client and returned transport in 48ms. `endpoint_rejected` is false. That row is `OBS-LOOPBACK-127-255`. Loopback policy was left as it is.

One-byte `a` and `e` occur in static Display and Client Debug words (`status`, `PARSE`, `gateway`, `redacted`). `message_has_token` is false for both, and the messages are the omission markers. The final `marker_a` hint is `slow down` (`hint_len` 9), which does not contain `a`.

`slow down` and `retry later` are preserved when they do not contain the secret (`static_message_preserved`: message length 9, hint length 11).

## Erratum

a089 `HANDOFF.md` line 16 says `AUTHORS` is not a MIT license body. That sentence is false. The delivery blob of `r-efi` 6.0.0 `AUTHORS` is 3733 bytes, sha256 `d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111`, git blob `79c8b7c4a1dabcfbc718b7fe6d43b1892f9850cd`. It contains the MIT grant. `r-efi` 5.3.0 `AUTHORS` is 3693 bytes, sha256 `ff92bed461f50338dd703a9ba9aee496a425957873df1d91192776e4bdf5dda7`. Both hashes match a078. The body is in `ERRATUM.md`.

a084 says `AUTHORS` is not a license locator under the basename rule (`LICENSE`, `LICENCE`, `COPYING`, `NOTICE`). That rule, and `LGPL-2.1-or-later`, stay pending. No CDLA allow was added. `SUP-CDLA-TARGET-ALL` remains a084's cargo-deny exit 4. This review did not rerun deny or audit.

## Unsigned

`BearerToken` says dropping the value does not wipe process memory. This review does not claim zeroization. The crate docs say a client timeout means this process stopped waiting. This review does not claim a PostgreSQL cancel. Real Gateway, CLI, PostgreSQL, amd64, OpenAPI, and the rest of SP09-W01 stay unsigned. `SUP-DUNCE-OR-POLICY` and the aws-lc conjunctive obligations stay with the dependency work.

## Cleanup

UUID `9c6debdc-6167-4bcb-8824-239b51d37201` only. Removed containers `oo-a090-probe-…` and `oo-a090-msrv-…`, network `oo-a090-…-net`, volumes `…-cargo` and `…-target`, and `/tmp/oo-a090-…`. Each docker command exited 0. Afterwards own containers, volumes, and networks for that label were 0. No prune. Foreign builders were left in place. Record: `cleanup.txt`.
