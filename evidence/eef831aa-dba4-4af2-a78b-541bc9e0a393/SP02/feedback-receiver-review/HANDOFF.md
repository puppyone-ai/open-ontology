# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a393`. One reviewer. No subagents and no worktrees. The author process was not restarted.

## Owned path

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a393/SP02/feedback-receiver-review/**`

`tools/feedback-receiver` is the readonly product at `3be6264f18811b2ad88eedf986c342b15bdd0a16`. Root manifests, `tools/audit-ingest`, `tools/selfbuild-suggestions`, and `deploy/trusted-demo/pg` were read and not modified. The ledger was not modified.

## Status

Verdict in `REPORT.md`: **APPROVE_LIMITED** for the queue slice. `FINDINGS.md` lists the frozen checks and the bounds. Raw HTTP is under `http/`. Phase exits are under `linux/proof/phases/`. PostgreSQL privilege and SQLSTATE files are under `pg/`.

## Rerun

`linux/proof/launch.sh` on a host that already has the two pinned images. It refuses to pull. It archives the fixed commit, mints a new secret directory, and uses network `oo-a393-net`, containers `oo-a393-pg` and `oo-a393-proof`, and volumes `oo-a393-pgdata`, `oo-a393-cargo`, and `oo-a393-target`. One builder, 6GiB, 1 CPU, including the launch. One PostgreSQL 17.10, 1GiB, 1 CPU. `RUSTUP_TOOLCHAIN=1.94.0`. Do not `cargo update` or `cargo generate-lockfile`. Do not prune. Do not remove another task's containers or volumes.

A passing run deletes the secret directory, the volumes, the network, and both containers. A failed run keeps the volumes, the network, and the secret directory. Copy `linux/proof/phases` aside before a second launch. `raw/console.log` appends.
