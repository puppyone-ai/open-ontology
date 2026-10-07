# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a355`. Slice E50-D1/1, S6–S8 candidate production.

## Recorded results

- Smoke rustc before edits: 1.94.0, `smoke-before/rustc.exit` recorded with `smoke-before/rustc.txt`.
- Focused crate tests: `build/test.exit` 0. Library 3 passed. Producer 7 passed, 0 failed, 1 ignored (`build/test.txt`).
- Clippy `-D warnings`: `build/clippy.exit` 0, empty `build/clippy.txt`.
- rustfmt: `build/rustfmt.exit` 0, `build/rustfmt-check.exit` 0.
- query-pg from fixed base `7a6f7eac8b8041eae215c087d1a80930437871ce`: `querypg/fetch.exit` 0, `querypg/build.exit` 0, `querypg/libclang.exit` 0. Binary SHA-256 `68ddb8ecdb031cff8db5b0764f3916c0593aad43eda2158257b1c5667a365c30`.
- Private Postgres 17.10: `pg/up.exit` 0, `pg/apply.exit` 0, apply log ends `shadow ready`.
- Live ignored test, collected after the container had already exited: `live/test.exit` 0, container `oo-a355-sbs-live` exit 0. `gateway_probe_is_refused_before_one_admitted_select` passed in 0.20s (`live/test.txt`). Full predicate list is `live/record.txt`.

The passing live test requires this order: probe `UPDATE shop.account SET label = label` exits 6 with empty stdout, then one `SELECT party_key FROM shop.account WHERE party_key IS NOT NULL LIMIT 1` is admitted (`returned_rows >= 1`, summary `succeeded`). `gateway.admitted` is 1. The second example stays `not_run`. `knowledge_verified` is false. Effect flags stay closed.

No product defect remained open against those recorded commands. Source was not changed in this continuation. The live test was not launched again.

## Failed attempts

1. `failed-attempts/build-1`: `Number::from_str_exact` is absent on serde_json 1.0.151. Clippy and rustfmt were not installed for toolchain 1.94.0. Fixed by `text.parse::<Number>()` and `rustup component add` inside that builder. Later suite passed.
2. `failed-attempts/build-2`: `CARGO_BIN_EXE_selfbuild_suggestions` is not a compile-time env. Clippy `type_complexity` and `collapsible_if`. Fixed with a runtime binary lookup, a `RawTable` struct, and let-chains.
3. `failed-attempts/build-3`: the wide multi-column fixture was unsealed, so the refusal was `seal` rather than `catalog`. Clippy `collapsible_if` in `walk_text`. Fixed by sealing through `link_command`.
4. `failed-attempts/querypg-1`: `cargo fetch --locked -p oo-gateway` rejected `-p`. Fetch line is now `cargo fetch --locked` with no `-p`.
5. `failed-attempts/querypg-2`: `oo-gateway/build.rs` panicked, `sha256sum failed` for `tools/gateway-token-store/src/cache.rs`, because the archive had only `Cargo.toml`, `Cargo.lock`, and `crates`. The retry archives `tools/gateway-token-store` from the same commit. Commit blobs match the 13 pinned hashes. `querypg/build.exit` is then 0.

A host redaction briefly matched the SAN heredoc in `pg/up.sh` because a small non-secret `san.cnf` shared that text. The four SAN lines were restored. The running certificate was created before that edit. `pg/server-san.txt` records `DNS:oo-a355-pg`. The redaction was not run again.

## Evidence limits

- Query-pg NDJSON is not stored. The exact saved record is `live/test.txt` together with the assertions and `run_gateway` predicates in `live/record.txt`.
- `knowledge_verified` is false after the admitted select. Admission is not certification and not semantic publication.
- Sealed-catalog `reltuples` were not a fresh scan. The profile tables array for that run is empty. The private `ANALYZE` covers the one mapped shadow table so the gateway `unknown_rows=refuse` policy can see a statistic. The 100k row set was not loaded.
- This directory contains no password values and no PEM private keys.
- The 18-minute handle wait and the single observation in `observation/handles.txt` were already done. They were not repeated. No new review count is recorded.

## Outside this slice

Parent-plan acceptance, SP13 goal acceptance, frontend sessions `a351` / `a353` / `a352`, root manifest edits, and semantic publication were not done.
