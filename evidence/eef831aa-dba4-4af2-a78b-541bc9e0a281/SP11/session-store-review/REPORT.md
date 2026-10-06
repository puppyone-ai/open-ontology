# Session store independent review

Verdict: **APPROVE_LIMITED** for the PostgreSQL adapter in `tools/session-store` at `e213ce37140090e843ba98c188009f920962937d`, tree `6b63c32d45fe973cf4671ca41c4ac827c4995636`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a281`. This directory is the independent review of frozen RT-D03 session-store. The author record is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a275/SP11/session-store`. That record is not this review. The reducer review `eef831aa-dba4-4af2-a78b-541bc9e0a272` is APPROVE_LIMITED for the pure reducer at `b0f7da4771e3c3625a45329c721e174f4380571c` and does not review this adapter. No product file, root lock, connector manifest, spec, or ledger was edited. This file does not accept parent RT-D03, does not close SP11, and does not certify a physical release.

## Decisive run

`linux/proof-3`, container `oo-a281-proof3`, id `3808862ec9d0662170cc10940ed46f20408796736c31c299ad5f2f3059795203`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a281-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`. `capacity.txt` recorded `own=0 global=0 host_cargo=0 own_postgres=1 cap_global=10 cap_own=1 postgres=1`. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `repo-head.txt` at launch is `5dd7b11a7c7bbbb94e6b16afc82228e82f645275`. The build used `git archive e213ce37140090e843ba98c188009f920962937d` of `Cargo.lock`, `tools/session-store`, and `tools/session-transition`. Reducer directory tree `a25733870f1ffac55c057dd399270905ccca9151`.

`docker-run.exit` 0. `version.exit` 0. `blob-cmp.exit` 0. `overlay-absent.exit` 0 before the smoke. `lock-subset.exit` 0, `lock-compare.exit` 0, `lock-summary.exit` 0. Summary line: `summary registry_triples=128 missing=0 mismatch=0 direct_pins_present=true path_packages=2`. Path lines: `session-store 0.1.0`, `session-transition 0.1.0`. `fetch-offline.exit` 0 on the already filled private cargo volume. `fetch.exit` 0. `lock-recipe.txt` is `committed lock; cargo fetch --locked; no generate-lockfile; no cargo update`. `smoke-test.exit` 0, `smoke-count.txt` 23, `smoke-names.exit` 0. `overlay-present.exit` 0. `test.exit` 0, `overlay-count.txt` 8. `clippy-component.exit` 0, `clippy.exit` 0. `product-unchanged.exit` 0.

Smoke stdout, before the overlay existed in the work tree:

- library: `ok. 11 passed; 0 failed` in 0.00s
- live: `ok. 12 passed; 0 failed` in 2.35s

The 12 live names required by `inside.sh` are the author names: `install_replay_drift_partial_and_privileges`, `wrong_ca_does_not_connect`, `two_connections_race_one_winner`, `rollback_before_commit_leaves_the_row`, `new_connection_replays_and_conflict_refuses`, `commit_unknown_reconciles_without_a_second_transition`, `owner_lease_takeover_keeps_the_workspace`, `changed_principal_new_session_and_project_separation`, `project_key_and_principal_are_not_authentication`, `column_json_mismatch_is_not_repaired`, `cli_acks_refusals_and_hides_the_password`, `product_does_not_read_the_fixture_directory`. The overlay name `a281_session` was absent from that stdout.

Clippy was `cargo clippy --locked --offline --all-targets -- -D warnings`. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.59s`.

## Fresh session counts

`linux/proof-3/test.stdout`, eight tests, 2.63s, `8 passed; 0 failed`:

- `a281 bind revision=1 generation=1 journal=1 stored_now_ms=4200 server_ms=1791274484910`
- `a281 cas applied=1 stale=1 revision=3 journal=3 winner=ev-a281-turn`
- `a281 retry outcome=idempotent revision=3 journal=3`
- `a281 conflict reason=event_id_conflict revision=4 generation=2 journal=4 fingerprint_match=1 reconcile=durable`
- `a281 privileges insert=42501 delete=42501 manifest=42501 other_revision=0 auth_revision=1`
- `a281 swapped_fingerprint reconcile=durable retry=event_id_conflict revision=1 updates=0`
- `a281 generation_column reconcile=consistency column=7 json_generation=1 repaired=0`
- `a281 commit_unknown reconcile=durable revision=1 journal=1 fingerprint_match=1 conflict_updates=0`
- `a281 rowlock session_store_backends=1 waiting=1 revision=1 journal=1`
- `a281 migration hash_drift=1 partial_tables=0 rebuilt=installed`
- `a281 cli usage=1 json_marker_leaked=0 connect_marker_leaked=0 applied_revision=1 stale=2`

The fingerprint compared with the journal is the canonical SHA-256 returned by `session_transition::transition` on the same event. The overlay does not reimplement the hash.

`linux/proof-2` ran the same sequence before the row-lock observer was corrected. Its race line is an independent sample: `a281 cas applied=1 stale=1 revision=3 journal=3 winner=ev-a281-beat`, then the same idempotent retry and `event_id_conflict` at revision 4 generation 2. One winner in each race. The revision moved by one.

## Same transaction

`transition_with_fault` opens one client, calls `begin` (`client.transaction()`), loads with `sql::LOCK_HEAD` (`SELECT ... FOR UPDATE`) on that transaction, calls `session_transition::transition`, and writes with `sql::UPDATE_HEAD` on that same transaction only when the reducer outcome is `Applied`. `Idempotent` and `Refused` commit the read. There is no second pool.

The row-lock test holds `session_head` with an admin `SELECT ... FOR UPDATE`, then starts one runtime transition. A second runtime connection, the same role as the adapter, sees one `application_name=session-store` backend whose `wait_event_type` is `Lock`. After the admin `ROLLBACK`, the receipt is `Applied` revision 1, journal length 1, `last_now_ms` 4200.

## Reconcile contract

`CONTRACT.md` says the event id is durable when it is present in the stored journal, and a consistency mismatch is `consistency`. `reconcile` takes the event id and returns the row's current revision and owner generation. It does not receive a fingerprint.

A stored journal hash replaced with 64 zero hex digits still loads, because the numeric columns still match the JSON. Reconcile of that event id is `Durable`. The original payload retried under that event id is `event_id_conflict`, revision stays 1, and `updates=0`. A real conflicting payload on `ev-a281-bind` after the generation change is also `event_id_conflict` at revision 4 generation 2, with `fingerprint_match=1` on the original journal entry. The same event id with expected revision 0 and generation 0 is `idempotent` because the journal check precedes the stale check.

Setting the `owner_generation` column to 7 while the JSON keeps 1 makes reconcile, get, and transition return `Consistency`. The column stays 7. The JSON fragment `owner_generation":1` stays. The adapter does not repair the row.

That is the written contract. A conflicting fingerprint is not accepted as `idempotent` or `applied`.

## Identity, clock, privileges

`now_ms` stored on the bind is 4200. The server clock read in the same test is `1791274484910`. The takeover `last_now_ms` is the stored lease expiry from the operator clock, which the test bounds below 100000.

The bound principal is `operator-cedar`. The runtime login is `oo_session_runtime`. A principal equal to that role name is refused. Runtime cannot install. Admin cannot transition. Runtime `INSERT`, `DELETE`, and manifest `SELECT` return SQLSTATE `42501`. A second project row stays at revision 0 while the authorized project reaches revision 1. Project key mismatch is a refusal. This is row separation, not an authenticator. `auth_provider_used` and `physical_release_certified` stay false on the adapter receipt and on the nested reducer. Adapter `database_used` is true. The local reducer call used as the fingerprint oracle keeps `database_used` false.

Lost `COMMIT` reply (`CommitFault::DropCommitResponse`) returns `commit_unknown`. A fresh connection reconciles that event id as `Durable` at revision 1, journal length 1, and the stored fingerprint matches the reducer. A conflicting retry does not append.

A damaged migration hash is install drift and is left damaged. Restoring the hash allows replay. Dropping the schema and leaving an empty schema is drift, `session_head` stays absent, and a later install on a dropped schema returns `installed`.

The CLI rejects an extra argv with exit 1. A static marker placed in the JSON password field is absent from stdout, stderr, `/proc/pid/cmdline`, and `environ` for a malformed document and for a wrong password. Install exit 0 and an applied transition exit 0 also leave the real password out of those streams. A stale `begin_turn` exits 2.

## PostgreSQL

One private server, container `oo-a281-pg`, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/up.exit` is 0. `pg/ports.json` is `{}`. Network `oo-a281-eef831aa-net`. CapDrop `["ALL"]`, CapAdd `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. The fixture directory is mounted read-only at `/run/oo-a275/fixture` for the author tests and at `/run/oo-a281/fixture` for the overlay. Endpoint host `oo-a281-pg`, database `oo_a281`. Private keys stayed under `/tmp/oo-a281-eef831aa-dba4-4af2-a78b-541bc9e0a281` and were not copied into this evidence directory. `oo-a276-pg` was already running and was not stopped.

## Lock recipe that was not copied

Author `linux/impl-7/inside.sh` still contains `cargo generate-lockfile` at line 73 and `cargo update -p "$name" --precise "$precise"` at line 121. `a275` `FINDINGS.md` records `linux/impl-1`: `generate-lockfile` selected newer transitives than the fixed archive, `lock.exit` 4, and no product compile. Later author attempts reconciled with precise updates. That contradicted the frozen no-update instruction. The committed `tools/session-store/Cargo.lock` is what this review fetched. `inside.sh` here has no `generate-lockfile` and no `cargo update`. The first proof's empty cargo volume failed offline fetch with 101, then `cargo fetch --locked` exited 0. The subset check runs before fetch and exits before fetch on a mismatch.

## Preserved harness attempts

`linux/proof`, container `oo-a281-proof1`, id `c6509775ce0e6234c698df2338c04762647bb6cfd54eaf6d0fa94ee5c59cbe67`, `docker-run.exit` 1. Smoke passed, count 23. The overlay failed to compile: `i64::from` does not accept the `u64` operator instant at `tests/a281_session.rs:389`. `test.exit` 101. `clippy.exit` 101 for the same error. `product-unchanged.exit` 0. The failing source is `linux/proof/failed-overlay-a281_session.rs`, SHA-256 `252fc8a4059f701944d7160f3b621813d4d437095fef31b3420fede76e3ec59d`.

`linux/proof-2`, container `oo-a281-proof2`, id `2e1358b78c9b28073367b42dca42cfe1deaf4d8b22a7d1e21bd877662b1ea962`, `docker-run.exit` 1. Smoke count 23. Overlay `7 passed; 1 failed`. `row_lock_is_one_backend_transaction` panicked with `session_store_backends=1 waiting=0`. The admin role can see the other backend's `application_name` and cannot see its `wait_event_type`. Clippy exited 0. `product-unchanged.exit` 0. The failing source is `linux/proof-2/failed-overlay-a281_session.rs`, SHA-256 `45bf5434405ac302cd458721e2dff59e2fa38c4206680a5d5cfcd0f59050247b`.

Those consoles and exit files were not rewritten.

## Image contents that were not an oracle

`python-present.flag` is 1 in each proof. The script recorded `command -v`. No Python process hashed a crate, a row, or an expected value. The fingerprint oracle is `session_transition::transition`. The inventory tool is the std-only `tools/sha256_manifest.rs`.

## Limits

The journal cap stays 1024. This adapter does not raise it. Public authentication is not implemented. Reconcile is scoped to event-id presence. A generation column that disagrees with the JSON is `consistency`. `physical_release_certified` stays false. The caller supplies the clock and the identity tuple. This approval is the adapter behavior exercised by `linux/proof-3` plus the author smoke on that same archive. It is not a parent acceptance.
