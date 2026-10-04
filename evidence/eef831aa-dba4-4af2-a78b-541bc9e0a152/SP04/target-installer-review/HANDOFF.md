# Handoff

APPROVE LIMITED the schema installer at `4ed22b8c1189e72b06b20a45335860ac9a3ea44e`, tree `fafb0fe6752b8240eb2b29025fc38bccdfb7144d`, parent `0791e971f49cd8afc646d80179241ddbcfaa1a1c`.

Read `REPORT.md`, `FINDINGS.md`, and `linux/probe/live/outcome.txt`. The deciding oracle is `ok. 2 passed` in 15.75 seconds. Smoke on the same archive is `ok. 9 passed` and scoped Clippy exit 0. Attempt 1 is a harness miss, kept under `linux/probe/attempt-1/`.

## Closed here

`install(&mut PgConnection)` and `verify_readiness(&mut PgConnection)` for one fresh database and for the forward routes in the `target-installer/v1` contract: replay, direct v1 without a v2 marker, v2 history retained, v3-without-mapping running `0004` only, `OO001`, `OO002`, `OO021` rollback, and runtime verify-without-install. Embedded digests match the frozen SQL. `0002` is not executed.

## Open, and not signed by this verdict

A second empty database on a cluster that already has `oo_target_owner` or `oo_pipeline_runtime` is not installed. The observed result is `OO002`, no `_oo` schema, and `NotInstalled`. `0000` was not changed. Do not describe this as readiness for every target database.

SP04-W05, SP04-W05-a, AT-PL-05, Loader, CDC, mirror, ACK, TRIM, fenced `COPY`, publications, and source-reader admission stay unsigned. `_oo.schema_ready`, `_oo.segment_contract_ready`, and `_oo.segment_preflight_ready` still take a caller digest.

## Do not reopen

Do not edit `0000` through `0004` to make a second database look fresh. Do not treat the reference `psql` applicators as this installer. The a108 and a142 SQL approvals were reused because the blobs are unchanged. They were not rerun.

Slot B is the disjoint reader-activation review. This session did not move connector code.

No stage of product files, no push, and no release from this review. Cleanup of the a152 containers, network, volumes, and private `/tmp` directory is in `CLEANUP.txt`.
