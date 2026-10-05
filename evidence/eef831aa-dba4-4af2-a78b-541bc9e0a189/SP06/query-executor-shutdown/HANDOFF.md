# Handoff — a189

## Result

Repair of the existing trusted `SELECT` executor. Source `15d23fe9f2cbc74e3f3278d70d0f56390568126e`. Parent `c77013d5c67830ff4a504eba096c84f645d41727`. Baseline `8a289f0c699abb8c707ca45ad45cb036abc3fc05`. a184 review evidence `df6067e9c6098f874b857d53fec139b78d6107c8`.

`DedicatedSession::execute_trusted_select` binds the first syntactically valid role before `guard_select` and before `admit`. A different valid role on that session is `RoleReuse`, including when the first request was refused by the guard or by admission. An invalid role does not bind. The `pg_roles` flag check still runs inside the read-only transaction. A flag failure closes the socket, and the next call is `SessionClosed`.

A dropped stream whose database call is already polled cancels that stored backend and keeps the future polled until the server answers or 750 ms passes. On PostgreSQL 17.10 the lock-blocked drop returned in 3 ms and the `pg_sleep` drop returned in 6 ms, both after 3100 ms still active, both `partial`, SQLSTATE absent, rollback confirmed. The baseline lock drop on the same fixture shape waited 5703 ms and completed `failed` with SQLSTATE `57014`.

The Linux 1.94 pre-edit smoke exited 0 (library 10, clippy). The deciding proof exited 0 (library 11, clippy `-D warnings`, live test 1 passed in 15.51 s). `UNTRUSTED_ADMISSION` stays `Blocked`.

## Do next

Review this repair against `15d23fe9f2cbc74e3f3278d70d0f56390568126e` and `FINDINGS.md`. Leave SP06-W04, SP06-W05, and SP06-W06 open. Do not add an HTTP route, a pool, or an untrusted statement entry in order to publish this function.

The 2000 ms wall cases in `live/oracle.txt` completed `failed` / `Timeout` with SQLSTATE `57014` at 1796 ms and 1798 ms. On this fixture the server timer answers before the client sleep. The SQLSTATE-absent client-deadline arm is `Hot::Deadline` in `await_hot`. This oracle did not time that arm.

The fixture `note_oid=16669` and `fixture.page_hold` are the live catalog for this test. They are not a docs rule R4 whitelist.

## Review

Grok 4.7 in this session wrote the repair, ran the one builder and the one database, and wrote this record. No subagent was used. No release is claimed. No parent checkbox was changed.

## State left behind

Own containers `oo-a189-smoke`, `oo-a189-repro`, `oo-a189-proof`, and `oo-a189-pg`, volumes `oo-a189-cargo`, `oo-a189-target`, and `oo-a189-pgdata`, network `oo-a189-net`, and `/tmp/oo-a189-eef831aa-dba4-4af2-a78b-541bc9e0a189` are removed (`cleanup.txt`). The exited container `oo-a190-rust194` was left in place. Failed attempts remain under `linux/probe/attempt-1/`, `linux/proof/attempt-1/`, and `linux/proof/attempt-2/`. `docker prune` was not used.

`crates/oo-gateway/src/lib.rs` and the parser-worker files were outside this slot and are not in the source commit. This evidence directory is the record for `15d23fe9f2cbc74e3f3278d70d0f56390568126e`.
