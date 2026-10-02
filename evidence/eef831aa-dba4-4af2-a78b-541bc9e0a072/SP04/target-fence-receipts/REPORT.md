# SP04-W05-a target fence and apply receipts

This slice adds the target-side fence and apply-receipt schema for a later bounded import. It does not finish SP04-W05, AT-PL-05, the Loader, CDC, or the PostgreSQL vertical.

## Source

Baseline smoke archived commit `0bba2a7993e6b53b1bb7c587e6e978bc6f8133dc`, parent `1b22f960af6118fcdf8222f53aea6f75831c0cc9`, tree `93aedfb3a6b126cbf0c9e7bb6833e41bb7d14f3e`. That pin is in `pre-smoke/invocation.txt`. `pre-smoke/baseline-head.txt` contains `2b68c89934d4ac1a73467ef563b14a895d6c3778` because the launcher also recorded `HEAD` after the archive pin, and `HEAD` had moved. Those baseline files were left as written.

Post-implementation smoke archived commit `78d106fcb3e7e1705b617ed6df58933d85297e7b`, parent `a45c13c0e231dcbf6c7b97050b9bfbe8a11bb571`, tree `66010b8c215df498afbc585b5461b6668ae58cd9`, then overlaid the untracked pipeline migrations, tests, and target-fence contract. The dirty working-tree `Cargo.lock` and other crates were not in that snapshot.

## Schema

Contract `oo.pipeline.target-fence-receipts/v1`.

| Version | File | SHA-256 |
|---|---|---|
| `target-bootstrap/1` | `crates/oo-pipeline/migrations/0000_bootstrap_roles.sql` | `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208` |
| `target-fence-receipts/1` | `crates/oo-pipeline/migrations/0001_target_fence_expand.sql` | `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9` |

Database `oo_agent`, schema `_oo`. Migration executor lock `6404202605`. Shard locks use `lock-key/v1` and are not that key. Clock is `clock_timestamp()`. Roles `oo_target_owner` and `oo_pipeline_runtime` are `NOLOGIN`. The owner is not granted to the runtime. `PUBLIC` execute is closed on the fence functions, on `set_config(text, text, boolean)`, and on `pg_advisory%` and `pg_try_advisory%`.

Durable logged tables are `_oo.target_generations` and `_oo.apply_receipts`. Receipt identity is `(source, table_generation, txn_or_batch_id)`. The row stores `config/v1`, `type-policy/v1`, `batch/v1`, five SHA-256 digests, a closed position codec and lineage, a non-negative scale-0 position, completion flags, an optional segment bitmap, the fence shard and generation, and the database commit time. Reject events are not stored. A control-database lease is not read.

The same identity with the same payload, codec, lineage, position, and completion flags returns `replay` and does not change `target_commit_at`. A different payload, codec, or lineage raises `OO010` and leaves the original row. Fence generation is stored and is not part of that equality.

`adopt_target_generation` holds `pg_try_advisory_lock` on the calling backend. Renewal keeps the generation only for that backend, owner token, and session nonce. Another token on a backend that already holds the lock raises `OO017`. A new backend increments the generation. The session nonce lives in `pg_temp.oo_target_fence_session`, which another connection cannot see. `commit_apply_receipt` checks the lock and the generation row again in the same call. A committed assert does not authorize a later transaction.

## PostgreSQL evidence

Two owned PostgreSQL 17.10 fixtures were used, both image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, platform `linux/arm64`, no host port, no privileged mode, `no-new-privileges`, capability drop `ALL`, and the five capabilities the entrypoint needs: `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`.

The first container was `oo-a072-pg1710-7c1a0e2b-4f63-4b0a-9c2d-11a072fence01`, id `6e06dfe98bc3d8c1d6ff4e7990661113072611f86e2a3fe6a666001defeef0c4`. Its identity is in `pg/first-fixture/`. The resumed suite on that database passed after an in-place function replace. Evidence is `fixture/harness.log` through `fixture_result=pass`.

The published file then changed because a shared trigger referenced `NEW.generation` while inserting into `apply_receipts` (`42703`). Preserved stderr is `fixture/attempts/fence-flow-new-field/`. The generation comparison is now nested inside the `target_generations` branch. A second container, `oo-a072-pg1710-2c451bc0-b35f-4f27-beac-d8d2d3987a40`, id `e98b7c71058dec8de8dd81cebb03e0b1427bf1d1cef6eacf2287ef4ccc41c2d0`, applied the corrected files from empty. `fixture-fresh/` is that run. `fixture_result=pass`. Server version was PostgreSQL 17.10 on `aarch64-unknown-linux-gnu`.

From-empty results in `fixture-fresh/`:

- Before bookkeeping, repeat raised `OO002` (exit 3) and `schema_ready` returned `f`.
- After recording both checksum rows, a second apply did not duplicate them.
- A wrong expand checksum raised `OO001` (exit 3) and the rows stayed the same. `schema_ready` of the real checksum returned `t`. A mismatch returned `f` and did not change rows.
- `oo_a072_runtime` connected as itself and is not a superuser: `oo_a072_runtime|oo_a072_runtime|f`.
- Gateway connect refusal is `FATAL:  42501:` in `fixture-fresh/gateway_connect.server.log`. The psql client text has no SQLSTATE because the connection fails before `VERBOSITY` applies. The server log is the SQLSTATE record. Agent connect is the same code.
- Runtime `set_config`, advisory lock, insert, truncate, temp, `SET ROLE`, and `CREATE FUNCTION` raised `42501`.
- Owner insert, truncate, migration update, and receipt update raised `OO014`. Deleting the existing `shard_clock` row raised `OO014` and the row stayed `1|adopted|11111111-1111-4111-8111-111111111111`.
- Closed metadata left no receipt. Codec and lineage `OO015`, bad digest and overlong identity `23514`, fractional, huge, and negative positions `22003`, embedded NUL `22021`, bad partial and segment shapes `23514`, generation `-1` and `0` `22003`.
- Same-shard adopt while the lock was held raised `55P03`. `shard_left` and `shard_right` were both generation 1 at the same time.
- Another connection that knew the owner and generation raised `OO011`. The same backend with a different token raised `OO017` and stayed generation 1.
- A stale generation and a wrong owner raised `OO013` and wrote no receipt (`0|0|0`).
- A committed fail raised `OO018` and left `1|failed|...`. The next adopt returned 2. A rolled-back fail released the lock, the later receipt raised `OO011`, and the next adopt returned 2.
- Terminating the holder returned client exit 2. The next adopt advanced the generation. Receipt count for that batch was `0|0|1`.
- Crash before `COMMIT`: visible counts stayed `0|0|0` after `pg_terminate_backend`. Crash after `COMMIT`: counts stayed `1|1|1`.
- Generation `9223372036854775807` rejected the next adopt with `22003` and stayed `9223372036854775807|failed`. A decrease from 4 raised `22003` and the lock keys stayed the same (`OO019` on a key change).
- Conflicting payload, lineage, and codec raised `OO010`. Side rows inserted in the same transaction were absent afterwards (`0|0`). The stored receipt bytes did not change. Replay after takeover left the receipt in place and the shard at `2|adopted|22222222-2222-4222-8222-222222222222`.
- The migration executor lock raised `55P03`, then `protocol_repeat` succeeded. Forced migration failures raised `OO003`. `failed_fresh_probe` was `t|t`. Follow-on version `target-fence-receipts/2` was not committed.
- Login password scan reported `secret_scan=clear`.

Relations were logged (`relpersistence=p`) and owned by `oo_target_owner`. Publication count was 0. `oo_control` and `oo_agent_bare` had no `_oo` schema.

## Rust evidence

Pre-smoke container `oo-a072-msrv-997bbcb7-a0e9-4140-8505-e0500a317358` exited 0. Phase exits in `pre-smoke/phases/` are all 0. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `RUSTUP_TOOLCHAIN=1.94.0`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

Post-smoke run `9ee5b5cc-643a-4949-ba7a-87471876b937`, container `oo-a072-msrv-post-9ee5b5cc-643a-4949-ba7a-87471876b937`, exited 0. `fetch.exit=0`, `test.exit=0`, `clippy.exit=0`, `clippy-component.exit=0`, `worst.exit=0`. `tests/target_fence_contract.rs` passed 1 test. Clippy used `-D warnings`. Host config: privileged false, cap drop `ALL`, no added capabilities, no host port, private bridge, tmpfs `/tmp`. `oo-pipeline` `src/lib.rs` and `Cargo.toml` were not edited.

## What remains open

`commit_apply_receipt` writes the receipt row and no user data. `fixture_synth` shows data, quality, and receipt in one transaction and is not part of the product migration. A receipt-only call stores a receipt with counts `0|0|1`. Arbitrary SQL from a role that later receives table grants is not fenced. The Loader driver is still required.

No CDC, control-database mirror, stream acknowledgement, Valkey, publication, or source `registration_allowed` change. The jobs control schema was not modified. A control lease is not a target fence. `pgident/v1` names of the form `q_` plus hex can exceed PostgreSQL's 63-byte identifier limit. This slice keeps fixture physical names short and does not emit DDL from unbounded logical names.

SP04-W05, AT-PL-05, and the vertical import path remain incomplete. This run is not a dual-architecture matrix, a 100-worker fault test, or a production deploy.
