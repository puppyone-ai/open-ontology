# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a289`. The store code is `tools/calendar-store`. The proof is this directory. The green run is `linux/impl-2`. The restart read is `linux/restart`. `linux/impl-1` is the preserved failure. `linux/precode` is the business-calendar smoke from before this overlay.

Fixed archive `216e6df82a61d2bc22548ce1a5075d5f4a045648`, tree `b7d5faef17d3e8f0170047cc7f82a996f46538ff`. Helper tree `ea5778dfb87a7cfdb04af2f236db78fe97f7bc4b`. This session did not edit the helper, `oo-core`, the root manifests, or the root lock.

Read `REPORT.md` and `FINDINGS.md` first. Product sources are in `source/blobs/tools/calendar-store/`. Raw exits are the `*.exit` files under `linux/` and `pg/`. `MANIFEST.sha256` is the Rust inventory of this directory and does not include itself. `linux/manifest/console.log`, `hostconfig.txt`, `docker-run.exit`, and `container-removed.txt` are written after that inventory.

## What the green run shows

One project, `project-one`, is bound at install. Snapshots keep the explicit `as_of`, calendar version, config digest, and declared closure provenance in `oo_calendar_control`. The helper runs again on the original YAML. Caller anchor dates are not stored as given. Exact replay keeps the row. Hash drift does not overwrite. Activation compare-and-swap refuses a stale pointer, and a revoked view-owner membership rolls the pointer update back with SQLSTATE `42501`.

`oo_dict.calendar` is owned by `oo_calendar_view` (NOLOGIN). The allowlisted runtime role can select it only after activate. Private config, closure labels, JSON, and the declaration hash are not in the view. The view owner cannot select `biz.receipts`. April, leap February, a Los Angeles civil day west of UTC midnight, and the São Paulo 2017-10-15 nonexistent midnight match the handwritten SQL literals. The reader sees the stored `as_of`. Those rows were still present after the postmaster restart.

PostgreSQL was one private 17.10 container, no host port, on `oo-a289-eef831aa-net`. The Rust builder was one arm64 1.94 container at a time, 6 GiB, 1 CPU, CapDrop ALL. Cargo home and the target directory were task volumes, not the host cache. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Child lock `a3c203e65af360f75cac875143a669e9ae91a73a02fa65d656db4d24db1cbe44`.

## What this handoff does not say

It does not accept SEM-D03 or SP08 for a parent. It does not certify a production release. It does not implement public authentication, a calendar owner UI, or a metric certificate. It does not authorize a shared database for more than one project. a283 reviewed the helper. a283 did not review this store. This session is the author of the store, not an independent review of it.

Docs/05 section 6.3 (`date_trunc`, `now()`, `current_setting('oo.as_of')`) is not the view this child installed. The contract records that refusal. a207 was not asked to change a manifest.

## Cleanup

`CLEANUP.txt` records removal of this task's PostgreSQL container, cargo volume, target volume, data volume, network, and `/tmp` secrets after the proof. The pinned images stay. `docker prune` was not run. Foreign containers and volumes were not removed. The manifest container is removed by `linux/manifest/launch.sh` after the inventory.

## Next child

A later child owns multi-project shared-database authorization, public gateway exposure, authentication, and any finance approval. This store keeps those flags false and keeps the view bound to the project recorded at install.

Do not add a project argument to `oo_dict.calendar`. Do not grant the view role access to business or source tables. Do not shift a gap or an overlap onto a nearby midnight. Do not overwrite a snapshot whose resolved hash changed.
