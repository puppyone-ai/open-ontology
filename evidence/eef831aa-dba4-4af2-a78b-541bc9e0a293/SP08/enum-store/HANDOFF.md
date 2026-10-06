# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a293`. The store code is `tools/enum-store`. The proof is this directory. The PostgreSQL contract is `linux/impl-1`. The green clippy run is `linux/impl-3`. The restart read is `linux/restart`. `linux/impl-1` and `linux/impl-2` also keep their failed clippy logs. `linux/precode` is the enum-history smoke from before this overlay.

Fixed archive `c8588faab992af1193ff58dbc489a0b117796aa0`, tree `f63d5ce2dcfbc8de759510280aa3761acf0b3e15`. Helper tree `701743f204495101b5efd20104bccb8d104df467`. This session did not edit the helper, `oo-core`, the root manifests, or the root lock.

Read `REPORT.md` and `FINDINGS.md` first. Product sources are in `source/blobs/tools/enum-store/`. Raw exits are the `*.exit` files under `linux/` and `pg/`. `MANIFEST.sha256` is the Rust inventory of this directory and does not include itself. `linux/manifest/console.log`, `hostconfig.txt`, `docker-run.exit`, and `container-removed.txt` are written after that inventory.

## What the green run shows

One project, `project-one`, is bound at install. Snapshots keep the explicit `as_of`, enum version, config digest, observations digest, threshold spelling, and report digest in `oo_enum_control`. The helper runs again on the original YAML. Exact replay keeps the row. A changed payload under the same identity does not overwrite. Activation compare-and-swap refuses a stale pointer, and a revoked view-owner membership rolls the pointer update back with SQLSTATE `42501`.

`oo_dict.enums` is owned by `oo_enum_view` (NOLOGIN). The allowlisted runtime role can select it only after activate. Private YAML, report JSON, notices, and published rows are not in the view. Runtime `UPDATE` of the view is SQLSTATE `42501`. The view owner cannot select `biz.receipts` or `snapshot.enum_yaml`.

The served historical row is code `4`, status `deprecated`, class `open`, description null, `as_of` `2020-01-01`, version `hist-2020`. Unknown code `9` was visible as status `unexplained` with a null class while the coverage snapshot was active. Superseded and approved rows from `retroactive.yaml` kept those statuses. Those private rows and the final historical view were still present after the postmaster restart.

PostgreSQL was one private 17.10 container, no host port, on `oo-a293-eef831aa-net`. The Rust builder was one arm64 1.94 container at a time, 6 GiB, 1 CPU, CapDrop ALL. Cargo home and the target directory were task volumes, not the host cache. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Child lock `19619c1de9ad8fec8a11b62e4464d93c3789a5ecfdc519281b2f0bc45970c923`.

## What this handoff does not say

It does not accept SEM-D04 or SP08 for a parent. It does not certify a production release. It does not certify finance, semantics, or an owner. It does not authorize a shared database for more than one project. a278 reviewed the helper. a278 did not review this store. This session is the author of the store, not an independent review of it.

Docs/05 section 6.3 is not the view this child installed. The contract records that boundary. a207 was not asked to change a manifest.

## Cleanup

`CLEANUP.txt` records removal of this task's PostgreSQL container, cargo volume, target volume, data volume, network, and `/tmp` secrets after the proof. The pinned images stay. `docker prune` was not run. Foreign containers and volumes were not removed. The manifest container is removed by `linux/manifest/launch.sh` after the inventory.

## Next child

A later child owns multi-project shared-database authorization and any finance, semantic, or owner certification. This store keeps those flags false and keeps the view bound to the project recorded at install.

Do not add a project argument to `oo_dict.enums`. Do not grant the runtime role access to `oo_enum_control` or to business tables. Do not rewrite superseded or deprecated status to `approved`. Do not invent a class for an unknown code. Do not overwrite a snapshot whose payload changed. Do not install a sweeper or a clock update.
