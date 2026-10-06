# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a275`. The adapter code is `tools/session-store`. The proof is this directory. The green run is `linux/impl-7`. `linux/impl-1` through `linux/impl-6` are preserved failures. `linux/precode` is the reducer smoke from before the adapter compile.

Fixed archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`. Reducer commit `b0f7da4771e3c3625a45329c721e174f4380571c`, directory tree `a25733870f1ffac55c057dd399270905ccca9151`. This session did not edit the reducer, jobs, core, root manifests, or the root lock.

Read `REPORT.md` and `FINDINGS.md` first. Product sources are in `source/blobs/`. Raw exits are the `*.exit` files under `linux/` and `pg/`. `MANIFEST.sha256` is the Rust inventory of this directory and does not include itself.

## What the green run shows

Install, exact replay, hash drift, and a schema without the manifest table. Runtime INSERT, DELETE, and manifest SELECT are rejected by PostgreSQL. Two connections race one expected revision. Rollback before COMMIT leaves the row. A new connection replays the same event and refuses a conflicting id. A hidden COMMIT reply is `commit_unknown` and reconciles by event id. Owner-lease takeover keeps the workspace. A changed principal needs a new session. A second project does not share the row. A revision column that disagrees with the JSON is not repaired. The CLI takes no arguments, hides the password, and uses the documented exit codes.

PostgreSQL was one private 17.10 container, no host port, on `oo-a275-eef831aa-net`. The Rust builder was one arm64 1.94 container at a time, 6 GiB, 1 CPU, CapDrop ALL. Cargo home and the target directory were task volumes, not the host cache.

## What this handoff does not say

It does not accept RT-D03 or SP11 for a parent. It does not certify a physical S1–S11 release. It does not implement public authentication or an IdP. It does not expand the 1024-event journal. a272 approved the pure reducer with limits. a272 did not review this adapter. This session is the author of the adapter, not an independent review of it.

`oo-pipeline` and `oo-connectors` were not linked. TLS behavior matches the gateway connector rules and is implemented in this crate. a207 was not asked to change a manifest.

## Cleanup

`CLEANUP.txt` records removal of this task's container, volumes, network, and `/tmp` secrets after the proof. The pinned images stay. `docker prune` was not run. Foreign containers and volumes were not removed.
