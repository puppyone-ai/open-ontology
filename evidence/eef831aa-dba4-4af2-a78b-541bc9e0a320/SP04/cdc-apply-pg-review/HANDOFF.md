# Handoff

Independent review of the CDC-D04 writer `tools/cdc-apply-pg` for session `eef831aa-dba4-4af2-a78b-541bc9e0a320`. Evidence is this directory. The product tree was not modified. The fixed commit is `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`, tree `539aa555a721a0b87b40546b9d4dad5569e3c863`.

Limited verdict: the fenced writer passed the preoverlay author suite and `linux/overlay`. Parent acceptance is false. SP04 stays open. CDC-D04 stays open. This review does not release the crate and does not mark the parent goal done.

Green runs:

- `linux/preoverlay`: author suite, 9 passed, 0 failed, Clippy 0, `lock_subset_ok tool_checksums=284`. Live test 1.94s. Scale test 14.85s.
- `linux/overlay`: `overlay/a320_apply.rs`, 2 passed, 0 failed, 20.15s, Clippy 0, same lock result. Container `3044d3a9309778f4de35e318884c8b781699b8daa1883f033db5c6e13266c9c0`.

No review attempt failed. The first overlay run is the green run. Author `linux/precode` and `linux/proof-1` through `linux/proof-7` stay under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a301/SP04/cdc-apply-pg`. Do not delete them. proof-4's sqlx `Display` still omits `OO013`; this review reads `DatabaseError::code()`.

The author's sentence that a299 spool approval is pending is older than the limited a299 record at `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. This review did not re-review the spool.

`adopt_target_generation` is durable before the write transaction. The first row failure left count 1, generation 1, no receipt, and qty 11. OO010, the pre-commit SIGKILL, and later refusals that reached adopt left the advanced generation in place. Printed generation values are in `linux/overlay/test.stdout`.

Callers must keep `ack_allowed` false. `source_cross_table_consistent`, `upstream_commit_lsn_verified`, `debezium_normalizer`, `p2_verified`, `p11_verified`, and `control_lease_consulted` stay false. `control_lease_valid` true does not bypass a fence held by another backend (`55P03`). `db_cross_table_consistent` covers this process's one local transaction only. Position provenance stays `caller_declared`.

Supported declared types in this run were `bool`, `int2`, `int4`, `int8`, and `text`, checked with `pg_type.typname`. `numeric`, `float8`, `jsonb`, and bare `int` returned `unsupported_type` before credentials. An int cell on a text column returned `type_mismatch`.

The tool lock has 284 registry checksums, all present in the fixed root, cdc-spool, and session-store locks. Do not `cargo update` it. Root manifests, migrations, and `crates/oo-core` belong to a313.

Still required before a caller treats a source transaction as acknowledged:

- ACK, XACK, TRIM, and a control-database mirror
- segmented apply above 200000 events
- source or WAL cross-table certification
- a Debezium normalizer
- router, segments, customer delivery, and release

Rebuild from a fresh evidence directory so existing `console.log` files are not overwritten. Use the pinned linux/arm64 Rust 1.94 image and the pinned Postgres 17.10 image, one builder, one private Postgres, 6 GiB, one CPU, and `git archive` of the fixed commit. The 200001-event feed belongs on `TMPDIR` outside the 256 MiB noexec `/tmp` tmpfs. The boundary source is `overlay/a320_apply.rs`.
