# HANDOFF — a104 segment preflight version

Stop here for fixed review. No push, pull request, release, or customer access.

## Read first

- `crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql`
- `crates/oo-pipeline/tests/segment_preflight_correction.rs`
- `contracts/pipeline/target-fence-receipts/target-fence-receipts-v3.json`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a104/SP04/segment-preflight/REPORT.md`

`0002_target_segment_null_expand.sql` stays. Its eager regclass cast is the reviewed `42P01` defect. The reference applicator does not execute it.

Assigned baseline `e84efd436dbc105b6a80df62464e1af7534e5af7`, tree `26612f1cf30dc1c08046915e961fd868f8cfc572`, parent `7da4e47e0f52890ded51e20df968cd3a198a1d2b`. It is an ancestor of the parent below.

Parent at commit: `3ca5357930a2c44c42d3061db5fabe577881269a`, tree `ec11cd3ee7948c33e6d43cd2e984e01b948f94e6`, subject `connectors: decode PostgreSQL values into Arrow 60 batches`. The delivery commit is the one whose tree contains this file. Confirm with `git show --name-only --format=fuller -1`.

`0003` SHA-256: `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3`.

Bytes that must stay:

- `0000` `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208`
- `0001` `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9`
- `0002` `59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512`
- v1 JSON `429faa9d32e4b42960e805e0883a2f67c8783d936e9094f0747e9c5e21a23521`
- v2 JSON `3a6ca1f4db777f98e8b76f0bd6cc6b5039f2e82d72c478ef4c8fe37713254bac`

## What the reviewer is checking

The reference route is executable in `route()` / `apply_reference`. From v1, including a healthy v1 database, it runs `0003` and does not run `0002`. An existing frozen v2 checksum is upgraded by `0003` only. v1 and v2 marker rows, their checksums, and lawful receipt bytes stay. A missing schema, relation, or segment constraint raises static `OO002` with message `target-fence-receipts/1 segment relation is absent` and inserts no marker. Running `0002` and then `0003` on a missing relation still raises `42P01`. That order is recorded as a dead path and is not a pass.

Version 3 is authorized only when the caller’s own SHA-256 of `0003` equals the digest for which `_oo.segment_preflight_ready` returns true. A v1 or v2 marker does not authorize it. A stored lie authorizes only the lie. The closed segment function and CHECK stay the `0002` bytes. Illegal history raises `OO021` and does not delete or rewrite receipts. Foreign owners of the replaced functions and views raise `42501` before replacement. There is no runtime grant of `oo_target_owner`.

## Evidence map

- `linux/baseline/` — `e84efd43` package smoke, run `e44357a9-a8ec-4ce3-a394-22e15537b631`, fetch/test/clippy/docker-run/worst exits 0. Rust 1.94.0 inside the container. No `0003` overlay.
- First fixed launch — `launch.sh` exit 101, 40.49s. Console was replaced by the rerun. Captured before replacement: fetch 0, package test 0, clippy 101 (four `-D warnings` errors), live test 101 at `phase_acl` with `42883` on the packed `commit_apply_receipt` signature. See REPORT.
- `linux/fixed/` — rerun `bfb4014f-6724-4b46-9359-0d98a0586b64`. Rust `1.94.0 (4a4ef493e 2026-03-02)`, PostgreSQL 17.10 aarch64. fetch/test/clippy/live/docker-run/worst exits 0. Live result `ok. 1 passed; 0 failed; 0 ignored`, 4.70s. `summary.log` ends `live=pass`.

No host compiler ran the deciding test. Capacity at the passing launch: `builder_containers=0 host_cargo=0 own_postgres=0`.

## Cleanup

Passing run removed `oo-a104-msrv-bfb4014f-6724-4b46-9359-0d98a0586b64`, `oo-a104-pg-bfb4014f-6724-4b46-9359-0d98a0586b64`, network `oo-a104-bfb4014f-6724-4b46-9359-0d98a0586b64-net`, and volume `oo-a104-pg-bfb4014f-6724-4b46-9359-0d98a0586b64`. Retained cargo and target volumes `oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104-cargo` and `oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104-target`. A later `docker ps --filter label=oo.owner=a104` printed no containers. No prune.

## Left alone

`0000`, `0001`, `0002`, both older JSON contracts, root manifest and lock, jobs, CLI, connectors, spec, plan, and older evidence. No `GRANT oo_target_owner TO oo_pipeline_runtime`.

## Not claimed

SP04-W05, AT-PL-05, Loader, CDC, control mirror, ACK, physical mapping, 100 workers, dual arch, and release. A085-F2, F3, and F5 remain open.
