# SP06-W06 local audit spool and ingest prerequisite

Session `eef831aa-dba4-4af2-a78b-541bc9e0a371`. This records the owned-path proof. It is not parent acceptance, not full SP06, not full SP02-W06, and not full gateway acceptance. The feedback receiver is not in this change.

## What was built

`tools/gateway-audit-wal` is a disk WAL for the existing `DedicatedSession` `AuditAdmission` hook. `admit` fsyncs before `BEGIN`. `complete` is a separate durable frame. The hook is the real executor hook. There is no second query executor and no manufactured `Admission` or `CompletionRecord`.

`tools/audit-ingest` is a separate rustls mTLS HTTPS receiver. It writes one PostgreSQL transaction for the event and the receipt, keyed by `(spool_id, request_id, event_seq)` plus the payload hash. The same bytes replay returns the same receipt. A different hash is HTTP 409. Workload identity is the client-certificate URI mapped through the project registry. HTTP headers are not identity. The receiver role is `INSERT` and `SELECT` only.

The gateway does not receive a control-database write credential. `wal-send` uses a client certificate and private key file to the collector only. Garbage collection runs only after a typed ack from the verified server matches the tuple and hash, and only for fully acknowledged sealed segments.

## Proof

One private PostgreSQL 17.10 and two local HTTPS processes (WAL sender and collector) inside one Linux arm64 Rust 1.94 builder. Namespace network `oo-a371-net`. No host network, no published ports, no docker socket, no host home, no shared cargo cache.

Images, already present, not pulled:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

Recipe files, checked at launch and left unmodified:

- `deploy/trusted-demo/pg/up.sh` `904aed7ed10156576252da4be7ad828832aefe4628d15f67c1436f475cad9bff`
- `deploy/trusted-demo/pg/pg-wrap.sh` `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`

The comment inside `pg-wrap.sh` cites an older hash. The proof uses the file-on-disk hash above.

Pre-edit smoke, before source edits: `preedit-smoke.txt`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `std::fs::File::try_lock` ok, exit 0. The first smoke shell used `bash -lc` and rustc was not on `PATH` (exit 127). The retry used `bash -c`.

Final builder: `rustc 1.94.0`, `cargo 1.94.0`, `clippy 0.1.94`. `RUSTUP_TOOLCHAIN=1.94.0`. `rust-toolchain.toml` still says 1.98.1 and was not edited. PostgreSQL `server_version.txt` is `17.10 (Debian 17.10-1.pgdg12+1)`. Public fixture `shop.entry` row count is 6.

Successful run, about 210 seconds, builder exit 0, inside exit 0. Phase exits, all `0`:

- `lock-wal`, `lock-ingest` (locks already present, not regenerated)
- `fetch-wal`, `fetch-ingest`
- `sha256-tool`, `select-debs-tool`, `libclang-extract`
- `build-ingest`
- `test-wal`, `test-ingest`
- `clippy-wal`, `clippy-ingest`
- `secret-scan`

`raw/immutable-before.txt` and `raw/immutable-after.txt` match. Gateway, core, control, jobs, root manifests, and the frontend were not edited.

`test-wal` on Rust 1.94, `--locked`, one thread:

- 10 library tests passed, including quota, sync rollback, torn tail, committed-frame corruption, and ack/GC prefix
- `untrusted_admission_stays_blocked` passed
- `dedicated_session_audit_is_durable` passed in 1.70s
- `child_locks_are_root_checksum_subsets` passed

`test-ingest`: three library tests passed (matching hash and project, headers are not identity, one URI SAN).

Clippy `--all-targets -D warnings` finished for both manifests.

Live result (`raw/live-result.txt`):

```
live=ok
privileges=ok
query=ok
offline=75
dedup=ok
restart=ok
gc=ok
conflict=409
stranger=403
force_project=retained
guard=ok
fsync=ok
quota=ok
enospc=ok
mode=ok
```

The query is the existing trusted-operator `DedicatedSession` path with auth false, role `oo_r_clerk`, through the original guard and parser, against the mapped public fixture. Six rows, exact decimal coefficients, scale 4. Admission was durable before the first result row. Completion is a second frame. Collector delivery, dedup, restart, and GC of the sealed segment ran against the real PostgreSQL schema. A caller JSON ack file did not authorize GC. A payload-hash mismatch returned 409. A stranger URI returned 403. A forced project header did not change identity and the segment stayed. Guard refusal did not admit. Injected fsync failure and the byte quota returned `AuditUnavailable` with no query transaction. Disk full used the private tmpfs `/oo-wal-quota` (262144 bytes), not the host disk, and returned `AuditUnavailable` with fault `no_space`.

## Source hashes

`SHA256SUMS` lists the 23 owned tool files. SHA-256 of that list file is `71c7ed9f404062096da910b538278be23ddf7fe7f4e48426620c062750f720af`. `SOURCE-INVENTORY.txt` is the same path list.

## Failed attempts

Phase logs under `linux/proof/phases/` are the successful run. Earlier failures are copied under `raw/`.

1. `raw/attempt-1`. Transaction API needs `&mut self`, and `rollback` takes `self`. `pg_query` bindgen had no libclang under `cap-drop ALL`.
2. `raw/attempt-2`. The exclusive lock file was moved before `try_clone`. Clippy warnings. `getrandom` error mapping without the `std` feature. A SQL comment contained the substring the ingest test forbids.
3. `raw/attempt-3`. `expect_err` needs `Debug` on `SelectStream`. The lock field looked unused. A headerless 3-byte file is corrupt, so the torn-tail fixture needed a real segment header.
4. `raw/attempt-4`. The builder followed `rust-toolchain.toml` and compiled 1.98.1. A local binding named `inspect` shadowed the function.
5. `raw/attempt-5-nospace`. After the uuid cast, the first collector send was `delivered=2`. The live test then failed at `tests/live.rs:643`: `last-fault.txt` was empty where `no_space` was required. `fs::write` truncates before it writes, and a full tmpfs has no free page for the new bytes.
6. `raw/attempt-6-kept-receipts`. The kept PostgreSQL volume already held two receipts. The next query inserted two more. The receipt query counted the whole table (4) instead of this spool (2).
7. `raw/attempt-7-lock-subset`. Live passed, then `lock_subset` failed: `cc 1.6.0` and other patch versions were not in the root lock. A 1.94 `generate-lockfile` reproduced those floats. The child locks were retargeted to root checksums. The root lock was not modified.

Earlier delivery failures, before attempt 5, were HTTP 500 from the collector: `tokio-postgres` treated `$1::uuid` as a uuid parameter, and `String`'s `ToSql` rejected it (`error serializing parameter 0`). The statements now cast `($1::text)::uuid`.

## Cleanup

`CLEANUP.md`: the successful run removed `oo-a371-proof`, `oo-a371-pg`, volumes `oo-a371-cargo`, `oo-a371-target`, `oo-a371-pgdata`, network `oo-a371-net`, and the private secret directory. No prune. Foreign containers were not touched.
