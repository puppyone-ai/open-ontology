# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a259`. Reviewed commit `e447249ab1a7f4c82df4729090b1285c9ef4a4ec`. Verdict APPROVE_LIMITED.

No product path was edited. a207 remains the connector manifest and root-lock writer. The evidence commit, when present, is only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a259/`. Nothing was pushed.

## Where to read the proof

- Contract blob in `source/blobs/reviewed-policy-quantity-v1.md`
- Archived adapter: `source/blobs/reviewed_policy.rs`, arm extract `linux/run/integer-arm.txt`
- Frozen tests: `linux/run/test-reviewed-policy.log`, `test-reviewed-policy-cli.log`, `test-reviewed-policy-quantity.log`
- Independent classify: `classify/policy.stdout`, `classify/classify.txt`
- Live generations: `live/generations.txt`, `live/outcome.json`, `live/reviewed-policy.stdout`
- Query: `live/steps/query-ticket.stdout`
- Failed attempts: `linux/run-attempt-1/` through `linux/run-attempt-3/`, each with `DIAGNOSIS.txt`
- Reviews: `reviews/01-adapter-cell.md` through `reviews/04-scope.md`
- Byte inventory: `SHA256SUMS` and `SHA256SUMS.sha256` from `tools/sha256.rs`

Successful top exit is `linux/run/run.exit` 0. Policy generation 48. Target fence 1. Query amounts are the int8 extremes and zero.

## Do not treat this directory as closure

`parent_acceptance` is false. `m1_i01` and `m1_i02` are `not_accepted`. `component_approval` is false. `development_unit` is false. Quantity is the existing adapter cell. This review does not add a component and does not accept M1-D14G.

## Next seams, each frozen before any further code

Money with a unit. Ratio, event time, date, audit time, geo, json, system. Float4, float8, numeric, and date codecs. Quantity on text or bool. Policy generation copied from the target fence.

## Resources

Cleanup removes only `oo-a259-pg`, `oo-a259-review`, `oo-a259-hash`, `oo-a259-hash-2`, volumes `oo-a259-eef831aa-cargo`, `oo-a259-eef831aa-pgdata`, `oo-a259-eef831aa-target`, network `oo-a259-eef831aa-net`, and `/tmp/oo-a259-eef831aa-dba4-4af2-a78b-541bc9e0a259`. Pinned images stay. No prune and no foreign cleanup.

The inventory hasher uses `--network none`, 6GiB, and 1 CPU. `linux/inventory/launch.sh` writes `SHA256SUMS`, then copies its console and hostconfig into `linux/inventory/`. `linux/inventory/rerun.sh` rewrites `SHA256SUMS` after those files exist. Its stdout and hostconfig stay under the private tmp path. That tmp directory is removed after the rerun.
