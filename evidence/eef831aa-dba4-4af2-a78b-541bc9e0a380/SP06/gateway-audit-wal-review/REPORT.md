# Gateway audit WAL review, original E55

Verdict: **APPROVE_LIMITED of original E55**.

The frozen child (Plan, 2026-10-08, E55-D1/1) is the local durability seam: an existing real DedicatedSession admission and completion on a bounded fsync WAL, then a real mTLS receiver that writes a durable PostgreSQL event and receipt, with GC only after confirmation. The checks named in that freeze are a real query, restart, collector-failure retention, pre-admission fsync failure with no result, and replay conflict. This packet is that seam.

Outside this verdict: the SP06 parent, full SP06, SP02-W06, public SQL, a feedback receiver, runsc, and a release. Plan, ledger, and acceptance counts were not edited. Local denominator text in the freeze (87 to 88, accepted 74/88) is the author's counting note. This review does not adopt it as a completion rate.

## Product

Fixed commit `1078d69e8c3cf8dacb145198f1e4123c767a196b`. `git diff --stat` of `tools/gateway-audit-wal` and `tools/audit-ingest` against that commit is empty, and no later commit touches those paths. `raw/product-fixed.txt` records the full hash. Launch HEAD in `raw/head.txt` is `8fc96b66d653c7e5a9a37f4a2ba03b40b804af0c`. That HEAD is a descendant. The two tool trees still match the fixed commit.

The author packet `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a371/SP06/gateway-audit-wal/` was read and not modified. SHA-256 of its `SHA256SUMS` is `71c7ed9f404062096da910b538278be23ddf7fe7f4e48426620c062750f720af` (23 files). The boundary oracle recomputed those 23 digests from `/repo` and recorded `product_sha=ok`.

`rust-toolchain.toml` remains channel `1.98.1`. The proof set `RUSTUP_TOOLCHAIN=1.94.0` and did not edit that file.

## Environment

One builder, `oo-a380-proof`: memory 6442450944, 1 CPU, pids 4096, `privileged=false`, network `oo-a380-net`, no published ports (`linux/proof/ports.json` is `{}`). Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. Repo bind was read-only. `CARGO_HOME=/cargo-home` and `CARGO_TARGET_DIR=/target` were private volumes. `HOME=/tmp`.

One PostgreSQL, `oo-a380-pg`: memory 1073741824, 1 CPU, pids 256, `privileged=false`, cap-drop `ALL` plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/server_version.txt` is `17.10 (Debian 17.10-1.pgdg12+1)`. `pg/rowcount.txt` is 6. No published ports.

Pre-edit smoke, before the `/tmp` overlay and before `cargo test`: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`, host `aarch64-unknown-linux-gnu`, LLVM 21.1.8, `try_lock=ok`. `preedit-smoke.exit` is 0. Clippy was absent on that toolchain (`clippy-version.txt`). `inside.sh` ran `rustup component add clippy` into the private toolchain. `raw/builder.log` then shows `clippy 0.1.94 (4a4ef493e3 2026-03-02)`.

Recipes were hash-pinned and not rewritten. Commit `10b78993641c88154c75c22219c79c1566e2cecd`. Digests are in `ORIGIN.md` and `raw/recipe-sha256.txt`. `pg-wrap.sh` is the entrypoint. `up.sh` was not executed. `extract-libclang.sh` was the deploy file, invoked unchanged. `raw/capacity-before.txt` listed no rust builder. Foreign PostgreSQL containers `oo-a377-pg` and `oo-a373-pg` were already up and were not removed.

Child `Cargo.lock` files were already present (`lock-wal.exit` 0, `lock-ingest.exit` 0). No `cargo update` and no `generate-lockfile`. `CARGO_NET_OFFLINE=true` after fetch and the libclang extract.

## Independent boundary

`linux/proof/boundary.rs` was copied onto a `/tmp` copy of `tools/gateway-audit-wal`. `raw/copy-diff.txt` is empty, so that copy matched `/repo` before the extra test was added. Symlinks pointed the crate at the read-only gateway and core trees. `OO_A380_BOUNDARY=1`. `OO_A371_LIVE` was unset.

`tests/live.rs` returns immediately unless `OO_A371_LIVE=1`. In `test-wal.log`, `dedicated_session_audit_is_durable` is `ok` and finished in 0.00s. That line is the skip. It is not this verdict.

`linux/proof/phases/boundary.log`: 1 passed, 0 failed, finished in 1.40s. `raw/boundary-result.txt`:

```
untrusted=blocked
product_sha=ok
source_order=ok
privileges=ok
admission_before_row=ok
query=ok
reader_restart=ok
completion_retained=ok
offline=75
gc_before_ack=0
project=shop
event_receipt_equal=ok
dedup=ok
restart_collector=ok
conflict=409
stranger=403
force_project=retained
header_ignored=ok
bad_hash_gc=closed
gc_after_ack=1
caller_ack_ignored=ok
guard=ok
fsync=ok
quota=ok
enospc=ok
mode=ok
boundary=ok
```

Admission before the query. `execute_trusted_select` contains `audit.admit(&admission).await?` before `self.client.take()`, and that function body contains neither `build_transaction` nor `open_read_only`. `open_read_only` contains `build_transaction()`. `DiskWal::append` calls `sync_active()` before `index.insert`. After `execute_trusted_select` returned, and before `next_row`, inspect showed one request in state `admitted`, an admission frame, and no completion frame. A new `wal-inspect replay` printed `state=admitted` and did not print `state=succeeded`. The SQL text is absent from `seg-00000001.wal`.

Query. Six rows from `SELECT entry_id, amount, wide FROM shop.entry ORDER BY entry_id` through the trusted-operator session, role `oo_r_clerk`, auth false. Coefficients `123456789012345678` and `12345678901234567890123456789012345678`, then `1`, `0`, with the sign pattern in `expect_row`, scale 4. Row 6 is an int `6` and two nulls. The session summary is `Succeeded` with `end_audit_ok`, and the spool request is `succeeded`.

Retention and the two restarts this run actually did. A second `wal-inspect` process replayed `state=succeeded` and `kind=completion` (`reader_restart=ok`, `completion_retained=ok`). With the collector down, `wal-send` exited 75, the output contained `retained`, and `seg-00000001.wal` was still a file. `wal-inspect gc` printed `removed=0`. After the collector was stopped and started again, a resend returned `delivered=2` and the same receipt rows (`restart_collector=ok`). The PostgreSQL server process was not restarted.

Identity. Client certificate SAN is `URI:spiffe://oo/workload/gateway-audit-wal`. Both delivered rows have `project=shop`. `handle()` contains `certificate_uri` and `prepare_event(&body`, and the function body does not contain `x-project`. `deliver.rs` sends the header `x-project: not-the-identity`. A stranger URI (`spiffe://oo/workload/stranger`) returned HTTP 403 and left the receipt rows unchanged. `force_project` made `wal-send` exit 75, left the segment in place, and left the count of events whose project is not `shop` at 0.

Event and receipt. `insert_event` has one `transaction()`, the event insert, `insert_receipt`, and one `commit()`. For this spool the counts are 2 events, 2 receipts, and 0 events without a receipt. Deleting `frontier.chk` and sending the same bytes again returned `delivered=2` and the same receipt rows (`dedup=ok`). A body with the same tuple and a different payload hash returned HTTP 409, and the counts stayed 2, 2, 0. A crash between the two inserts was not injected.

Ack before GC. Before any checkpoint ack, GC removed nothing. Flipping payload-hash byte 48 and repairing the checkpoint trailer made GC exit non-zero and left the segment (`bad_hash_gc=closed`). Restoring the checkpoint made GC print `removed=1` and delete `seg-00000001.wal`. A caller file `ack.json` containing `{"ack":true}` made GC print `removed=0`.

Quota and fsync, no query. `fail_next_syncs(1)` returned `AuditUnavailable`, the spool had no frames, `last-fault.txt` was `sync`, and `pg_stat_activity` for that backend had `xact_start` null and state `idle`. `max_bytes: 100` did the same with fault `quota`. Filling the private tmpfs `/oo-wal-quota` (262144 bytes) did the same with fault `no_space`. That tmpfs is not the host disk.

Also recorded on the same run: `audit_ingest` table grants are only `INSERT` and `SELECT`; the `feedback` table count is 0; `UPDATE` on `event` fails; `SELECT rolpassword FROM pg_authid` fails; the ingest role cannot connect to `oo_query`; the login role cannot connect to `oo_audit`. An `INSERT` is `GuardError::Write`. `SELECT hidden` is a guard error. The literal token is absent from the spool files. A send config created mode `0644` is refused (`mode=ok`). `UNTRUSTED_ADMISSION` is `Blocked`. Public SQL was not executed.

## Product tests on the same builder

Locked, offline, `--test-threads=1`, from `/repo`:

- `gateway-audit-wal` library: 10 passed, 0 failed.
- `untrusted_admission_stays_blocked`: 1 passed.
- `child_locks_are_root_checksum_subsets`: 1 passed.
- `audit-ingest`: 3 passed (`event_requires_matching_hash_and_project`, `headers_do_not_select_a_project`, `one_uri_san_is_required`).
- Clippy `--all-targets -D warnings` for both manifests: exit 0.

`raw/immutable-before.txt` and `raw/immutable-after.txt` match (`cmp -s`). The compared paths are the gateway executor, guard, lib, policy, both gateway manifests, the root `Cargo.toml` and `Cargo.lock`, and the wal store plus the ingest lib and `pg.rs`. `secret-scan.exit` is 0. `inside.exit` is 0. `builder.exit` is 0.

## Failed attempts

`raw/failed-attempts.txt`. Attempt 1 did not compile (`SelectStream` is not `Debug`). Attempt 2 panicked on a whole-file `x-project` search. Attempt 3 is the run above. The launch trap had written the word `none` over the attempt note. The attempt directories were already copied and are unchanged by that correction.

## Cleanup

`CLEANUP.md`: removed `oo-a380-proof`, `oo-a380-pg`, `oo-a380-cargo`, `oo-a380-target`, `oo-a380-pgdata`, `oo-a380-net`, and `/tmp/oo-a380-eef831aa-dba4-4af2-a78b-541bc9e0a380`. A later `docker ps` filter on `oo-a380` is empty, and that secret directory is absent. No prune. Foreign containers were not removed.
