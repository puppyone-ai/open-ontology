# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a326`. The passing proof is `linux/proof-3/` unless a finding names an earlier directory.

## F1. The embedded store is a private module

`oo-arch-check` rejects a path dependency on `tools/gateway-token-store`. `build.rs` pins the accepted bytes and compiles them as `accepted_token_store`. That module is private, so its public items are unused from the HTTP crate. `#[allow(unused, clippy::all)]` is on the generated module only. The accepted bytes are not edited. `linux/compile/clippy.stderr` is the failure before that allow.

## F2. The consumer lock keeps the workspace hyper edges

a319 F2 says the probe lock adds `httpdate` and drops `h2` and `want`. This card's lock does not copy that array. `linux/proof-3/lock-consumer.txt` shows hyper 1.11.1 `+httpdate` and no `-h2` or `-want`. Tower and tower-http gain only the edges named in `REPORT.md`. No existing name, version, or checksum changed. `denied=0`.

## F3. Package Clippy reached older gateway tests

`cargo clippy -p oo-gateway --all-targets -- -D warnings` compiles every target in the crate. Two failures were outside the new HTTP files and blocked that command:

- `tests/query_executor_pg_live.rs` `canon_len` did not match `Cell::Decimal`. The arm now uses the same length rule as `cell_bytes` in `query_executor.rs`. That file was not edited.
- `policy_catalog.rs`, `policy.rs`, and `tests/column_acl_set_pg_live.rs` built a one-element slice with `.clone()`. Clippy 1.94 wants `std::slice::from_ref`.

`bin/query-pg.rs`, `query_executor.rs`, `reviewed_policy.rs`, and `tests/a327_numeric.rs` stayed untouched.

## F4. Cargo's binary path uses the hyphenated target name

`std::env::var("CARGO_BIN_EXE_oo_gateway")` was unset at runtime (`linux/proof/`). `env!("CARGO_BIN_EXE_oo_gateway")` is unset at compile time (`linux/proof-2/`). The name that exists while the integration test compiles is `CARGO_BIN_EXE_oo-gateway`, the same form `parser_worker.rs` uses for its binary. Commit `51d08e59ddb3` uses that name. The live test then passed.

## F5. Smoke attempt 1 had no libclang

`linux/smoke/` exited 101 because `pg_query` bindgen could not find libclang under `cap-drop ALL`. `linux/smoke-2/` reused the a200 Debian extractor onto the private cargo volume and exited 0. Later builders reuse that extraction. They do not apt-get and they do not prune.

## F6. The HTTP boundary is the loopback process

The proved health body sets `authentication` to `loopback-http` and `external_mtls`, `oauth`, and `idp` to false. The live test checks that body. Nothing in this process terminates external client TLS, performs an OAuth exchange, or calls an IdP. Caller-supplied tuple fields are not authority. `POST /v1/sql` returns 501 and does not consult the token store.

## F7. What this card does not contain

Durable audit, row-level security, runsc, Redis, a 429 limiter, and a 503 breaker are not implemented. Readiness does not probe PostgreSQL. The pack loader checks the operator artifact's declared digests in memory. It does not re-run the pack compiler. Fixture packs in the live test are test artifacts, not a second compiler. `execution/decisions/gateway-http-dependency-baseline.md` was not edited and still says `UNAPPROVED`.

## Close-out read

Before the inventory hash, `source/proof-51d08e59/` was read again against commit `51d08e59ddb3`. `http_gateway.rs` reads one parameterized token row as `oo_token_runtime`, then calls accepted `lookup` with `DeclaredTransport::ExternalClient`. A warm tuple entry does not slide its 5 second deadline, and a failed lookup drops it. `POST /v1/sql` returns 501 and does not reference `execute_trusted_select`. Pack selection uses the derived project, role, policy version, and knowledge version. File text stays in the operator catalog. Proof logs contain no `ooct_` or `oost_` text. This read did not change product code.
