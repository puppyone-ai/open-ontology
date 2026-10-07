# cdc-apply-pg report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a301`. Child writer for one sealed normalized CDC spool against the existing target installer, physical mapping, fence, and `apply_receipts`. This report records the private proof. It is not parent acceptance, not CDC-D04 acceptance, and not a release.

Fixed dependency archive: `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`, tree `672f73fddb93878ae858aa74999061e8d824cc65`. The proof archives that commit. It does not read moving workspace dependencies. Root manifests and migrations were not edited. a313 remains the root dependency writer. a207 remains the connector manifest writer. a299 spool approval is still pending.

## Final proof

Directory `linux/proof`. One builder, container `8e129c76e0fc5b5bfa6a1609425c15ee3166a415ed364ec790c56aa4fd7ac552`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0` `aarch64-unknown-linux-gnu`. PostgreSQL stayed the existing `oo-a301-pg` on `oo-a301-eef831aa-net`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.

Host limits from `hostconfig.txt`: ExitCode 0, OOMKilled false, Privileged false, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000, NetworkMode `oo-a301-eef831aa-net`. Capacity at launch: own builders 0, global builders 0, host cargo 0, own postgres 1.

| step | exit |
| --- | --- |
| version | 0 |
| source | 0 |
| lock-subset-rustc | 0 |
| lock-subset | 0 (`lock_subset_ok tool_checksums=284`) |
| lock-stable | 0 |
| clippy-add | 0 |
| test | 0 |
| clippy | 0 |
| lock-installed | 0 |
| docker-run | 0 |

Commands were `cargo test --locked --offline -j 1 -- --test-threads=1` and `cargo clippy --locked --offline --all-targets -- -D warnings`. Tool lock SHA-256 `67d47b32d081d28504a3d0fe0fcb431c49c618dd1f5599fa188d6f8878e1e78f`, same bytes in `tools/cdc-apply-pg/Cargo.lock` and `linux/proof/Cargo.lock`. No cargo update and no generate-lockfile.

`test.stdout` counts: tls 2 passed, live `fenced_writer_on_private_postgres` 1 passed in 2.13s, local 5 passed, scale `over_limit_returns_needs_segments_before_connect` 1 passed in 15.60s. The vendor crate `sqlx-postgres` printed two unfulfilled `dead_code` expectations. The `cdc-apply-pg` Clippy exit is 0.

## What the live test exercised

Fresh private TLS, installer-owned catalog, and LOGGED physical tables. Runtime role is not superuser. The same dedicated connection adopts the advisory fence, checks the generation, writes the receipt, and writes the selected rows in one SQL transaction.

Covered on that database:

- Bare database returns `capability_absent` with generation count 0.
- A copied database with one inserted historical `target-fence-receipts/2` row of a non-matching checksum returns `migration_drift` before adopt. Generation count stays 0 on both databases. Installed migration rows were not updated or deleted.
- An unregistered column returns `mapping_absent` with generation count 0.
- One spool transaction updates `orders`, deletes and inserts `lines`, and inserts a second order. Results are int8 max, the prior note left in place for unavailable, int8 min, and SQL NULL. Receipt `cross_table_consistent` is true, `partial_txn` false, `txn_complete` true, both segment fields null, bitmap empty.
- Output `source_cross_table_consistent`, upstream LSN verified, Debezium, P2, P11, `ack_allowed`, and `control_lease_consulted` stay false. Provenance stays `caller_declared`.
- Exact replay returns `replay`, writes 0 rows, and keeps the same receipt timestamp and payload.
- Same batch identity with a different payload returns `OO010`. The stored receipt and the order quantity stay as first applied.
- Unavailable on a required insert returns `unavailable_insert` and does not insert the row.
- A second connection holding the fence returns `fence_held` / `55P03`. `control_lease_valid` true does not bypass it.
- A direct `commit_apply_receipt` at the other generation returns SQLSTATE `OO013`.
- Injected `div(1,0)` after the row writes rolls back the receipt and leaves the prior quantity.
- SIGKILL before commit leaves no receipt; a later apply inserts. SIGKILL after commit and a later apply returns replay of that receipt.
- A hidden COMMIT response reconciles on a new connection. With reconcile disabled the process returns exit 3 `commit_unknown` and does not claim the row applied; a later normal apply writes it once.
- 200001 events returns `needs_segments` before the password file is read.

## Preserved failures

| directory | result |
| --- | --- |
| `linux/precode` | smoke before the product overlay, docker-run 0 |
| `linux/proof-1` | `?` in a bool helper, then 14 lock checksums outside the fixed locks. docker-run 1, test 101, lock-subset 1. No Clippy |
| `linux/proof-2` | `tests/live.rs` compile: invalid tuple-index format, moved schema strings, missing `sqlx::Row`. docker-run 101, test 101, lock-subset 0 |
| `linux/proof-3` | drift setup `UPDATE` rejected by the immutable migration trigger. docker-run 101, test 101, lock-subset 0, lock-stable 0 |
| `linux/proof-4` | sqlx display of the stale fence error omitted `OO013`. docker-run 101, test 101 |
| `linux/proof-5` | tests 0, Clippy 101, `collapsible_if` in `capabilities` |
| `linux/proof-6` | tests 0, Clippy 101, `manual_unwrap_or` in `main` |
| `linux/proof-7` | tests 0, Clippy 101, `too_many_arguments` on the spool test helpers |
| `pg/up.exit` | 0. That process was reused. No second a301 PostgreSQL was started |

## Recipe origin

`linux/proof/recipe-origin.txt` records the copied launch, PostgreSQL, and manifest origins, their commits, paths, and SHA-256 values. Manifest tool copied byte-for-byte from a199: `tools/sha256_manifest.rs` SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`.
