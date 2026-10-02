# Handoff: jobs-lease-operations/1

Stop for an independent review. Do not treat this as driver, fence, or SP02 completion. Do not check parent plan boxes. Do not push.

## Review this

- `crates/oo-jobs/migrations/0003_job_lease_operations_expand.sql`, SHA-256 `5cb2e9030a479903ecc57ce32fc3ee851f00d146e02ab3902fcbe7ab09501034`.
- `contracts/jobs/lease-operations/` (`jobs-lease-operations-v1.json` SHA-256 `63009790833468ddb4affa766a8310893469f72895e958119ecc837b92a1365b`).
- `crates/oo-jobs/tests/lease_operations_contract.rs` and `lease_operations_live.rs`.
- `crates/oo-jobs/tests/fixtures/lease_operations/`.
- `crates/oo-jobs/migrations/README.md`.
- Evidence under this directory. Implementation commit `c312cae2daf75cfaa0b618066d709b9e529b3f95`, tree `19319751f04a7cb048bf3980a5408357d10acf58`, parent `4477f23205e989ac3c2207098efc369e6286cb98`. Task baseline `aeb351cfb3b8c31e305568cd655ed057d4afdbf5`.

Published 0000, 0001, and 0002 bytes are unchanged. The Rust crate manifest and `src/lib.rs` are unchanged. The stub still says the driver is not implemented here.

## Semantics the review should hold

Trusted `oo_jobs_runtime` calls the seven owner-created definers. The project argument is a filter, not authentication. Owner and migrator stay privileged. Functions do not `COMMIT`. Payload is opaque bounded JSON text with a top-level credential-key denylist. Nested or renamed secret keys are not scanned.

Claim uses `FOR UPDATE SKIP LOCKED`, database `clock_timestamp()`, and a fixed 30-second lease. Expired running is requeued without a new attempt, then claimed as a new attempt and generation, including when the worker name is unchanged. At the attempt cap the expired row fails with `lease_expired_attempt_cap` and cannot be resurrected. The cap budget is 8 finalizations per call. Generation overflow raises `OO012` before mutation and aborts that claim transaction. There is no API to backdate expiry, force a generation, or skip `max_attempts`.

Heartbeat extends 30 seconds only while the lease is running, bound, and unexpired. It does not write a task event. Success, failure, and retry require the same active predicate. Retry sets `run_at` on the server. Attempt increases on the next claim. Cancel of queued matches generation. Cancel of running matches worker and generation and returns the lease it cleared. It does not terminate a target backend. Repeated completion returns `terminal`. `already_applied` is not produced. No terminal owner column was added.

`jobs.service_readiness` does not authorize v3. The applicator must compare the file hash itself. SQL accepts a stored checksum that is not the file hash (`OO001` only on a later mismatch against the stored value).

## Oracle

`REPORT.md` lists each proved case, the SQLSTATE or row, and the evidence digest. Host live proof is `live/result.txt`. Linux 1.94 proof of the committed tree is `impl/`, exits 0, live test ignored inside the builder. Author-host default tests and clippy are `host/console.log`.

Residuals that stay open:

- Rust scheduler, NOTIFY, watch loop, config, auth, backup, target loader, and a 100-worker end-to-end run. The 100 sessions proved the claim primitive only.
- A068-F3: readiness SQL trusts the stored checksum. The lie case shows enqueue still works.
- A068-F4: runtime can still take advisory lock `6401202601`. The waiter observed `55P03`.
- A068-F5: 0003 refuses to adopt the seven names. It does not stop a superuser from planting other definers.
- a079 lock order still deadlocks. The owner session received `40P01`. The claim session committed. Receipt DDL did not close this.
- Schedule and `jobs.leases` column grants remain. Runtime cannot set `target_fence_certified`. A control lease is not a target-fence certificate. `fence_read` stayed `false`.
- Lost lease is status `lost_lease`. It is not proof that a target write happened.
- Docs/17 jitter is omitted on purpose. Caller TTL is rejected by not existing. Generation below 1 is `OO010`, not `lost_lease`.

## Rerun

Default `cargo test --locked --offline -p oo-jobs` does not need PostgreSQL. The live test is `cargo test --locked --offline -p oo-jobs --test lease_operations_live -- --ignored` and fails closed if `oo-a102-pg1710-jobs` is not running. The fixture container from this session has been removed. A reviewer who reruns the live test must create their own PostgreSQL 17.10 container with that name, local trust for the fixture logins, and a read-only mount of `crates/oo-jobs` at `/fixture`. Do not reuse customer data or put a password on argv.

Linux smoke command, on the implementation commit, was fetch, test, and clippy as recorded in `impl/invocation.txt`. `airgap_claim=no`.

## Cleanup

`cleanup.txt` records removal of `oo-a102-rust194-baseline`, `oo-a102-rust194-impl`, `oo-a102-pg1710-jobs`, networks `oo-a102-baseline-net`, `oo-a102-impl-net`, and `oo-a102-pg-net`, and volumes `oo-a102-baseline-cargo`, `oo-a102-baseline-target`, `oo-a102-impl-cargo`, `oo-a102-impl-target`, and `oo-a102-pg-data`. The a098 Postgres container was left running. No prune.
