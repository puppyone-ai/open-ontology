# Handoff

Author delivery of `oo-pipeline import-pg` and `PgConnector::prepare_snapshot` on `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a`, tree `82b5549ca70475ba6b59eb63879d4e8f0719617a`, parent `d1b4e86c6b52c30337fbb5281e934b6bc82419e7`.

The next session can review that commit. The command reads `oo.pipeline.import-pg/v1` from stdin, prepares a snapshot from the configured read-only session, and calls the existing `import_unpublished` on one runtime connection. Stdout is `oo.pipeline.import-pg-result/v1` from the returned `RunSummary`. The database row is the seal. The contract is `contracts/pipeline/import-pg-cli-v1.md`. The requirement record is `REPORT.md`. The fresh process oracle is `linux/live-6/out/success.stdout`. The factory oracle is `linux/live-6/prepare.stdout`. Smoke before the edit is `linux/smoke-before/`. Failed exits are listed in `COMMANDS.txt` and kept under `linux/build/`, `linux/build-2/`, `linux/live/` through `linux/live-5/`.

`PgSession::rollback` now uses the driver transaction manager so a second custom `BEGIN` on the same session can proceed. Depth 1 still sends `sql::ROLLBACK`. Details are in `FINDINGS.md`.

Reuse a159 (`476d21c7`) for the unpublished import campaign. Reuse a149, a153, and a152 for the reader and installer primitives this command calls and does not reimplement. This run did not repeat those matrices.

This file is not an approval of publication, an Agent grant, a full type certificate, CDC, a control mirror, a job, a stream acknowledgement, or parent SP04. Nothing was pushed. `CLEANUP.txt` records removal of this task's container, volumes, network, and `/tmp` tree. Foreign containers were left in place. `docker prune` was not run.
