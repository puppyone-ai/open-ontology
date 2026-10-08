# Handoff

Verdict for the coordinator: **APPROVE_LIMITED of original E55**.

Session `eef831aa-dba4-4af2-a78b-541bc9e0a380`. Product `1078d69e8c3cf8dacb145198f1e4123c767a196b`. Own evidence is only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a380/SP06/gateway-audit-wal-review/`. Grok was the only executor. No subagents and no worktrees.

Do not mark the SP06 parent accepted. Do not edit the Plan, the ledger, or the acceptance counts from this packet. Do not treat it as full SP06, SP02-W06, public SQL, a feedback receiver, runsc, or a release.

## Evidence map

- `REPORT.md`, `FINDINGS.md`, `ORIGIN.md`, `CLEANUP.md`, this file.
- `raw/boundary-result.txt` ends in `boundary=ok`.
- `RAW-EXITS.txt` and `linux/proof/phases/*.exit`. Successful inside and builder exits are 0.
- `raw/failed-attempts.txt`, `raw/attempt-1/`, `raw/attempt-2/`.
- `SOURCE-INVENTORY.txt`, `SHA256SUMS`, `SHA256SUMS.sha256`.
- `linux/proof/boundary.rs` is the independent oracle. It is not in the product tree.

## What a later reader can rerun

`linux/proof/launch.sh` on a host that already has the two pinned images. It refuses to pull. The successful run deleted the secret directory, both containers, the three volumes, and `oo-a380-net`. A later launch mints new secrets and a new PostgreSQL volume. Do not point it at another task's volumes or secret directory.

One builder, 6GiB, 1 CPU, including host cargo, rustc, install, and inventory. One PostgreSQL 17.10, 1GiB, 1 CPU. `RUSTUP_TOOLCHAIN=1.94.0`. Host rustc is not the proof. `inside.sh` compares the immutable gateway and root manifest hashes before and after. `CARGO_NET_OFFLINE=true` is set after fetch and the libclang extract.

## Owned cleanup already done

`CLEANUP.md` records removal of `oo-a380-proof`, `oo-a380-pg`, `oo-a380-cargo`, `oo-a380-target`, `oo-a380-pgdata`, `oo-a380-net`, and the private secret directory. Confirmed after the run: no container name matching `oo-a380`, and the secret directory is absent. No prune and no foreign cleanup.
