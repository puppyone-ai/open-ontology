# GW-D01 / SP06-W01 gateway token store

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a304`. Fixed archive `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`. This unit stores opaque Gateway token hashes and a five-second positive cache. It does not close SP06-W01, the GW-D01 parent, or a release.

## Result

Final proof `linux/proof` docker-run exit 0. Container `oo-a304-proof` removed. `OOMKilled=false`. Memory 6442450944. NanoCpus 1000000000. CapDrop ALL. Network `oo-a304-eef831aa-net`. Privileged false.

`linux/proof/console.log`:

- `subset_ok registry=125 path=gateway-token-store`
- `running 18 tests`
- `test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.51s`
- `PHASE clippy` then `Finished dev profile`
- `PHASE proof_ok`

`linux/proof/randomness.txt` is `distinct_hashes=true` and `quality_claim=false`. `phase/stop-window.txt` is `remaining_ms=4598`. Postgres was the private container `oo-a304-pg` (image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`). The builder image was `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, toolchain rustc 1.94.0.

Smoke-2, before any overlay, checked the fixed archive of `tools/session-store` and `tools/session-transition`. `linux/smoke-2/docker-run.exit` is 0. Smoke-1 is preserved: exit 101 because that archive omitted `session-transition`.

## What the crate does

`tools/gateway-token-store` is a standalone workspace, contract `gateway-token-store-v1`. `TrustedControlAdapter` is the only issuer and revoker. It logs in as `oo_token_admin`. `lookup` logs in as `oo_token_runtime`. Neither path accepts an IdP assertion or an mTLS certificate. `DeclaredTransport` is a value the caller supplies after its own transport check. This crate checks that the value matches the frozen sandbox and session binding.

Session tokens are `oost_` plus 32 OS CSPRNG bytes (`getrandom` 0.2.17, default features off). Client tokens are `ooct_` with the same width. The plaintext is returned once on the issue receipt. The database, the cache key, and `Debug` store or show the SHA-256 hex, or `PlainToken(redacted)`. Session idle is 30 minutes and the absolute cap is 8 hours. Client default is 1 hour and the maximum supplied TTL is 90 days. The positive cache TTL is 5 seconds of `Instant`. Pub/sub is not implemented.

Schema `oo_token_control` is created by `migrations/oo_token_control_v1.sql` inside a transaction. The migration hash is stored. A shape or hash mismatch is `install_drift`. Runtime may select and may update only `last_used_ms` and `idle_deadline_ms`. The final tests passed that privilege list on PostgreSQL 17.10, including a refused `GRANT INSERT` drift check and a successful column update.

## Preserved attempts

| Directory | Exit | What stopped |
|---|---|---|
| `linux/smoke` | 101 | Fixed archive omitted `tools/session-transition`. |
| `linux/smoke-2` | 0 | Pre-overlay session-store check passed. |
| `linux/proof-1` | 101 | `cargo fetch --locked` refused the unsorted hand lock. Subset check had passed at 128 registry triples. |
| `linux/proof-2` | 101 | `SystemWall.now_ms` needed `WallClock` in scope. Fetch had succeeded. |
| `linux/proof-3` | 1 | 18 tests passed. `cargo-clippy` was absent from the image. |
| `linux/proof` | 0 | Tests, Clippy, and the crate manifest. |

The proof-1 lock difference was read with `cargo metadata --offline` on a throwaway copy inside `oo-a304-lockdiag`. That command was not `cargo update` and not `cargo generate-lockfile` on the repo lock. The repo lock was then edited by hand: the package block moved before `getrandom`, and unused `itoa`, `serde_json`, and `zmij` plus the `serde` to `serde_derive` edge were removed. No version or checksum was added. `lock-diagnose.log` keeps the diff.

## Recipes

Origins and hashes are in `linux/proof/recipe-origin.txt`.

- a204 launch `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`. Changes: 6g, 1 cpu, jobs 1, global cap 10.
- a200 proof launch `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`.
- a200 `pg/up.sh` `96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45`. Changes: database `oo_a304`, alias `oo-a304-pg`, statement logging off, builder fixture without private keys.
- a199 `sha256_manifest.rs` `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`, copied byte-identical into `tools/sha256_manifest.rs`.

## Cleanup

`cleanup.txt` records removal of `oo-a304-pg`, volumes `oo-a304-eef831aa-cargo`, `oo-a304-eef831aa-target`, and `oo-a304-eef831aa-pgdata`, network `oo-a304-eef831aa-net`, and `/tmp/oo-a304-eef831aa-dba4-4af2-a78b-541bc9e0a304`. No prune. After that, those names were absent.

## Inventory

`linux/inventory/launch.sh` compiles `tools/sha256_manifest.rs` in one pinned builder and, when that run exits 0, writes `MANIFEST.sha256`. The tool skips that output path. `linux/inventory/scope.txt` names the host files written after the walk. Those files stay outside the listing. Before the walk, the same binary lists `tools/gateway-token-store` and compares it to `linux/proof/crate-manifest.sha256`.

## Limits

The crate does not implement public HTTP authentication, an IdP, OAuth, PKCE, refresh, RFC 8693, an mTLS handshake, Redis or any pub/sub, an audit WAL, agent SQL, session-lease certification, or query cancel. A warm cache entry can stay positive until its original 5 second expiry. The injected clock is a test seam. Production `issue` and `lookup` use `SystemTime`. Root `Cargo.toml` and root `Cargo.lock` were not modified. a207 remains the connector manifest and root lock writer.
