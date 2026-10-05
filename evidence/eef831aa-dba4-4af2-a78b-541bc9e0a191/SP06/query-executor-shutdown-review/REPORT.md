# a191 report — fixed executor shutdown review

Task `a191`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a191`. Repository `/Users/redux/puppy-one/open-ontology`.

Verdict: **APPROVE LIMITED**. Contract `contracts/gateway/query-executor-v1.md` (`oo.gateway.query-executor/v1`). SP06-W04, SP06-W05, and SP06-W06 stay open. This review does not approve a release and does not mark a parent item done.

Fixed source `15d23fe9f2cbc74e3f3278d70d0f56390568126e` (`gateway: run one permission-bound SELECT on a dedicated session`). Parent `c77013d5c67830ff4a504eba096c84f645d41727`. Tree `feec821ab54f0aaf0001218f90150befce2803b2`. Baseline `8a289f0c699abb8c707ca45ad45cb036abc3fc05` is an ancestor. Author evidence `c3ac02007d279a4c149bfe70e1486de103cb4c08` (a189). Prior review evidence `df6067e9c6098f874b857d53fec139b78d6107c8` (a184, APPROVE LIMITED).

The review read the three a189 reports, the a184 report, the fixed diff against the baseline, `docs/07-gateway.md`, `docs/17-rust-implementation.md` section 2, and the contract. No `AGENTS.md` is present. Slot C is this review. Slot B is the a187 parser-worker writer. Slot A is the a190 policy-catalog writer. Those slots were left untouched. Product sources, manifests, locks, gates, and ledgers were left untouched. `git archive` of `15d23fe9` was extracted under `/tmp` and was not a branch move.

## What the limited approval covers

On a reachable PostgreSQL 17.10 TLS session, dropping a stream that has already been active for 5.2 s cancels that backend in under 1 s. The lock-blocked drop returned in 10 ms. The `pg_sleep` drop returned in 9 ms. Both were still active immediately before the drop. Both completed `partial`, stop class `Protocol`, SQLSTATE absent, rollback confirmed, end audit recorded, and the same pid plus `backend_start` was gone from `pg_stat_activity`. Socket drop is recorded and is not the proof.

The same run separates four stops. A server `statement_timeout` keeps SQLSTATE `57014` and stop class `Timeout` (2289 ms, rollback confirmed). A `CancelHandle` user cancel keeps SQLSTATE `57014` and stop class `Canceled` (rollback confirmed). A client deadline after the first portal page has SQLSTATE absent, stop class `Timeout`, and rollback not claimed (8047 ms, one row already returned). A `complete` hook failure leaves status `partial` and `end_audit_ok` false.

Role bind matches the contract. Invalid shape, identity mismatch, row limit, and a non-whole-millisecond wall return before any bind. A guard refusal binds the role and writes no audit row. A later different valid role is `RoleReuse` and writes no audit row. The same role can run again. An `admit` failure binds and writes no audit row. A `pg_roles` flag failure closes the socket; the next call is `SessionClosed` and adds no audit row.

`UNTRUSTED_ADMISSION` remains `Blocked`. The fixture functions `a191_hold`, `a191_brief`, and `a191_pace` are volatile and are not an R4 whitelist. The audit hook is a second client's autocommit insert and is not a production WAL. The a184 typed-row, star-column, truncation, OID refusal, lying-catalog, and guard-display campaign was not repeated.

## Limit recorded with the approval

`send_bound_cancel` and `CancelHandle::cancel` both await `cancel_query` with no deadline. In `await_hot`, the 750 ms poll of the pinned database future starts only after that await returns (`query_executor.rs` lines 1196-1215 and 1241-1255). Contract lines 49-51 place that 750 ms ceiling on the pinned database future and the following `ROLLBACK`, after the cancel call. The healthy-server drops above sit inside that reading.

A focused stall of the cancel TCP, and only that TCP, kept `Drop` blocked at the 1502 ms observation point while the original backend was still `active` (`live/oracle.txt`, `fault_drop_blocked=true`, `fault_cancel_accepts=1`). The producer holds the client for that whole await, and `reap_finished` returns `SessionBusy` while the task is running, so this path does not lend the connection out. The 750 ms clock had not started. That is the limit: the cancel connection itself is outside the cleanup ceiling. This run treats it as contract-matching residual, recorded so a later slice does not describe `cancel_query` as a 750 ms operation.

The same stall on a `CancelHandle` let the brief query finish, then let the next generation become active, then released the held TCP. At 1500 ms after release the next statement was still active (`fault_race_next=still-running`). `cancel_query` returned `Sent` on the join that starts after that snapshot. The snapshot does not prove a packet already on the wire is harmless. The contract's required late-cancel fence did hold on the healthy server: a finished handle returned `NotSent`, and the next statement stayed active.

## Archive and smoke

`linux/smoke/archive-verify.txt` is `blobs=26868 hashed=26868 missing=0 mismatch=0`. Blob ids came from `git ls-tree` and `git hash-object`. The compare counts those ids. File digests came from the Rust `sha256` tool (`linux/smoke/owned-sha256.txt` and `linux/proof/owned-sha256.txt`). `source/blobs/` holds `query_executor.rs`, `query_executor_pg_live.rs`, `lib.rs`, and the contract. `source/COMMIT` and `source/TREE` name `15d23fe9` and `feec821ab54f0aaf0001218f90150befce2803b2`.

Pre-overlay smoke, before the review test was copied onto the archive:

- Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`.
- Container `oo-a191-smoke`, id `332c9d4b246d685abbe44337a216420e7ef6bb172f8b5c3adab1187ffaecfcbd`. `linux/arm64`, `--pull never`, `--cap-drop ALL`, `CapAdd` null, privileged false, memory 8g, `PortBindings` empty, `HOME=/tmp`, network `oo-a191-net` (`289afdee96e49e9bda2fdc5bd2d8c145f15909273b2f795a04d5e21ac8158e1a`, bridge, `internal=false`).
- `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`.
- `linux/smoke/smoke.exit` is 0. Library tests 11 passed, 0 failed (`Finished test profile in 36.17s`). Clippy `-D warnings` on `--lib --test query_executor_pg_live` finished with no warning (`23.86s`). `overlay-absent.txt` is `lib.rs=fixed-archive overlay=none`.
- Preserved misses: `linux/smoke/attempt-1/` (exit 127, `/tmp` is `noexec`), `attempt-2/` (curl exit 52), `attempt-3/` (incomplete clang root, libz3 absent).

## Live proof

One PostgreSQL 17.10 container `oo-a191-pg`, id `df255cc41343ed6b9e9364f6c86a0ef3e30bc4575e1f385189e7bfbf202334d7`, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `arch=arm64`. Privileged false, `CapDrop ALL`, `CapAdd` `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. `pg/ports.json` is `{}`. `pg/up.exit` is 0. Server version in the oracle is `17.10 (Debian 17.10-1.pgdg12+1)`, `server_ssl=on`. The certificate SAN covers `oo-a191-pg` and `oo-a191-proxy`. Passwords and keys stayed under `/tmp/oo-a191-eef831aa-dba4-4af2-a78b-541bc9e0a191/secrets`.

Deciding proof container `oo-a191-proof`, id `eb96f37045171675b32c2d6c72f76bb6d4a6e5ed60d20594c16d952fc0aba53a`, same Rust image and network, `CapAdd` null, privileged false, memory 8g, no published port. `linux/proof/capacity.txt` is `host_cargo=0 rust_builders=0 global=0 cap=3`. The overlay was copied onto the archive only at this launch, after `smoke.exit` was 0.

`phases/lib-test.exit`, `clippy.exit`, and `live.exit` are 0. Library tests 11 passed. Clippy `-D warnings` on `--test query_executor_a191_review` finished in 0.45 s with no warning. `review_fixed_executor_shutdown` passed, 1 passed, 0 failed, 31.11 s. The deciding oracle is `live/oracle.txt`.

`proof.exit` is 1 because the in-container leak scan matched its own source. `leak-scan.txt` is `leak_hits=0` and `private_key_markers=1` at `tools/leakscan.rs`. `linux/proof/leakscan-self-hit.txt` records that. Preserved proof misses, none of which ran the fixture: `attempt-1/` and `attempt-2/` (borrow errors in the overlay), `attempt-3/` (`clippy::while_let_loop` in `finish_ok`).

## Cleanup

Own containers, volumes, network, and the `/tmp` tree for this UUID are removed after the final Rust scan. `cleanup.txt` holds the command exits. Other tasks' containers and the pinned images stay. `docker prune` was not used.
