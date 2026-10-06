# Findings

## No product defect in the frozen adapter

`linux/proof-3` compiled the archive at `e213ce37140090e843ba98c188009f920962937d` with the committed `tools/session-store/Cargo.lock`. Registry subset: 128 triples, missing 0, mismatch 0, both path packages allowlisted. Author smoke 11 library and 12 live tests passed before the overlay was copied. The eight overlay tests passed. Clippy `-D warnings` passed. `product-unchanged.exit` is 0.

The conflicting-fingerprint check held. After a real apply, a different payload under the same event id is `event_id_conflict` and the revision, generation, and journal length stay put. After a SQL swap of `canonical_sha256` to 64 zeroes, reconcile by that event id is `Durable`, which is the contract's event-id rule, and the original payload retried through `transition` is `event_id_conflict` with `updates=0`. A column `owner_generation` of 7 against JSON generation 1 is `Consistency` for reconcile, get, and transition, and the row is not repaired.

The same-transaction check held. One runtime backend waited on `Lock` while another transaction held the row `FOR UPDATE`. After that holder rolled back, the waiting call applied once: revision 1, journal length 1.

Lost COMMIT reply is `commit_unknown`. A new connection reconciles that same event id, and the stored fingerprint matches the reducer output for that event.

## Historical author lock deviation, preserved and not repeated

`evidence/.../a275/SP11/session-store/linux/impl-7/inside.sh` line 73 runs `cargo generate-lockfile`. Line 121 runs `cargo update -p "$name" --precise "$precise"`. `a275` `FINDINGS.md` records `linux/impl-1` exit 4: `generate-lockfile` selected newer transitives than the fixed archive, the first precise pin of `js-sys` still conflicted with `web-sys` 0.3.106, and no product compile ran. Later author runs used precise updates to reach the lock that was committed.

This review does not copy that recipe. `linux/proof-3/lock-recipe.txt` records committed lock, `cargo fetch --locked`, no `generate-lockfile`, and no `cargo update`. The subset tool fails the run before fetch when a triple is missing or mismatched, when a direct pin is absent, or when a path package other than `session-store` or `session-transition` appears. `a272`'s first parser rejected root path packages and the script still fetched. This `inside.sh` does not continue into fetch after a subset failure.

## Preserved review harness failures

`linux/proof` `docker-run.exit` is 1. Smoke count is 23. `test.stderr` is `error[E0277]` at the overlay line `assert!(server_ms > i64::from(takeover_now))`. `takeover_now` is `u64`. `i64` implements `From` for `u32` and not for `u64`. Clippy reported the same compile error. The product archive was unchanged. The failing overlay is saved as `linux/proof/failed-overlay-a281_session.rs`.

`linux/proof-2` `docker-run.exit` is 1. Smoke count is 23. Seven overlay tests passed, including the CAS, conflict, privilege, fingerprint, generation, commit, migration, and CLI counts quoted in `REPORT.md`. `row_lock_is_one_backend_transaction` observed `session_store_backends=1 waiting=0` for the whole poll. The observer was `oo_session_admin`. PostgreSQL leaves `wait_event_type` null on sessions whose role the caller is not a member of. The runtime backend stayed connected until the admin transaction rolled back, which is why the count stayed 1. Clippy on that tree exited 0. The failing overlay is saved as `linux/proof-2/failed-overlay-a281_session.rs`.

`linux/proof-3` observes `pg_stat_activity` as `oo_session_runtime`, the adapter's login. That run records `session_store_backends=1 waiting=1`. A separate probe inside `oo-a281-pg`, deleted before the reports, showed the same split: a runtime session sees another runtime session's `wait_event_type=Lock`, and an admin session sees `<null>` for that runtime row. The probe script was removed from the container. It is not part of the evidence tree.

## Contract limit, recorded as behavior

Reconcile durability is event-id presence plus a consistent snapshot. It will report `Durable` for an event id whose stored `canonical_sha256` was overwritten, as long as the numeric columns still match the JSON. The transition path then refuses a conflicting payload and does not rewrite the row. Callers that need the fingerprint must compare the journal hash on a successful get. This review does not treat that as a break of `CONTRACT.md` lines on get and reconcile.

## What stayed false

`auth_provider_used`, `runtime_used`, and `physical_release_certified` stayed false on the receipts the overlay checked. The adapter receipt sets `database_used` when it used PostgreSQL. The nested reducer flags stay false. The local fingerprint call is the reducer alone and keeps `database_used` false. No authenticator ran. No physical-release certificate was claimed.

`python-present.flag` is 1 because the Debian image has `python`. It was not used to compute an expected value.
