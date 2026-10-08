# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a371`. Grok was the only executor. No subagents and no worktrees.

## Owned paths

- `tools/gateway-audit-wal/**`
- `tools/audit-ingest/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a371/SP06/gateway-audit-wal/**`

Gateway, core, control, jobs, root manifests, frontend, and the reviewed deploy recipes were read and not modified.

## Status

The local durability seam and the ingest prerequisite have a passing private proof. See `REPORT.md` and `raw/live-result.txt`. Do not treat that as full SP06, full SP02-W06, collector-feedback, or parent acceptance.

Source list: `SOURCE-INVENTORY.txt`. Hashes: `SHA256SUMS` (SHA-256 of that file: `71c7ed9f404062096da910b538278be23ddf7fe7f4e48426620c062750f720af`).

## What the next reader should rerun

`linux/proof/launch.sh` on a machine that already has the two pinned images. It refuses to pull. It mints secrets only when `login.pass` or `client.key` is missing. The successful run deleted the secret directory, the volumes, the network, and both containers (`CLEANUP.md`). A later launch therefore mints new secrets and a new PostgreSQL volume. Do not point it at another task's volumes or secret directory.

The builder must set `RUSTUP_TOOLCHAIN=1.94.0`. Host `cargo` and `rustc` are not the proof. One builder, 6GiB, 1 CPU. One PostgreSQL 17.10, 1GiB, 1 CPU. Global builder cap is 10.

`inside.sh` compares hashes of the immutable gateway files before and after. `CARGO_NET_OFFLINE=true` is set after fetch and the libclang extract. Do not `cargo update` the root lock.

## Integration surface

- Library: `DiskWal` implements `AuditAdmission`.
- Operator binaries: `wal-inspect` and `wal-send`. There is no public authenticated HTTP endpoint on the gateway.
- Collector binary: `audit-ingest`, with `migrations/oo_audit_ingest_v1.sql`.
- Contracts and wire schemas were frozen and were not rewritten: both `CONTRACT.md` files, `schema/event.v1.json`, `schema/ack.v1.json`, `schema/registry.md`.
