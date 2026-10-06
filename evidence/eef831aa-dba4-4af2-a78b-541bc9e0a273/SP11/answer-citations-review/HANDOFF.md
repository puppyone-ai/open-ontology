# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a273`. Reviewed commit `7e404dd3d36cdeecb553ab0c707dbfe82ba94c62`. Fixed archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`.

## Where to read the proof

- Contract and producer blobs: `source/blobs/`
- Review harness: `overlay/a273_review.rs`
- Baseline, before the harness existed on the archive: `linux/smoke/`
- Failed clippy, tests already green, harness preserved: `linux/proof/`
- Passing tests and Clippy: `linux/proof-attempt-2/`
- Origins: `origin/recipes.txt`
- Byte inventory: `SHA256SUMS` and `product-SHA256SUMS`, written by `tools/sha256.rs`

Successful top exits are smoke 0 and proof-attempt-2 0. Attempt 2 `test.exit` is 0 and `clippy.exit` is 0. Attempt 1 `test.exit` is 0 and `clippy.exit` is 101.

## Do not treat this directory as closure

`parent_acceptance` is false. This review does not mark RT-D02, SP11-W04A, or docs/08 section 7 done for the parent plan. It does not judge factual entailment. `local_operator_declared` is the request trust value. It does not make `authenticated_audit` true. `SummaryLine.turn` in query-pg is stdout only.

## Resources

Cleanup removes only this task's containers, network `oo-a273-eef831aa-net`, volumes `oo-a273-eef831aa-cargo` and `oo-a273-eef831aa-target`, and `/tmp/oo-a273-eef831aa-dba4-4af2-a78b-541bc9e0a273`. Exits are in `cleanup/run.log`. Pinned images stay. No prune and no foreign cleanup.

The inventory hasher uses `--network none`, 6g, and 1 CPU. `linux/inventory/launch.sh` copies its console into `linux/inventory/`. `linux/inventory/rerun.sh` rewrites the sums after that console and `cleanup/run.log` exist. The rerun console stays under `/tmp` and is removed with the task tmp directory.

## Next seam

Any broader citation rule, including numeric entailment or warning-prose review, needs its own frozen contract before code. This review did not edit `tools/answer-citations` or the root lock.
