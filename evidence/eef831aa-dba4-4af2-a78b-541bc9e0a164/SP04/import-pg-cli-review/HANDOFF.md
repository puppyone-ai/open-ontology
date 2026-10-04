# Handoff

Independent review of `oo-pipeline import-pg` and `PgConnector::prepare_snapshot` on `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a`, tree `82b5549ca70475ba6b59eb63879d4e8f0719617a`, parent `d1b4e86c6b52c30337fbb5281e934b6bc82419e7`.

Verdict: **APPROVE LIMITED**. The requirement record is `REPORT.md`. The line-level notes are `FINDINGS.md`. Author evidence `fe7e4741` is not this proof. Loader proof remains a159 `476d21c7`.

The deciding import oracle is `linux/probe-4/out/a164-success.stdout` and `linux/probe-4/out/a164-outcome.txt`. The deciding rollback oracle is `linux/probe-3/out/depth-outcome.txt`. Pre-edit smoke is `linux/smoke/`. Failed harness runs are `linux/probe/` and `linux/probe-2/`. `linux/probe-3/out/success.stdout` is the existing author fixture, written after the independent test by `import_pg_cli_live`; do not read it as the yard fixture.

The overlay under `overlay/` was applied only to a copy of the git archive. The product tree was not edited. `full_import.rs` was not edited.

This file is not an approval of publication, an Agent grant, a full type certificate, CDC, a control mirror, a job, a stream acknowledgement, or parent SP04. Nothing was pushed. `CLEANUP.txt` records removal of this task's container, volumes, network, and `/tmp` tree. Foreign containers were left in place. `docker prune` was not run. `MANIFEST.sha256` is the SHA-256 of every file in this directory except itself.
