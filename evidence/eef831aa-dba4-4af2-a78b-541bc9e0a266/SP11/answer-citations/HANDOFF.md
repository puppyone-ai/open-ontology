# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a266`. Fixed commit `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`.

## Where to read the proof

- Contract: `tools/answer-citations/CONTRACT.md`
- Library and CLI: `tools/answer-citations/src/`
- Tests: `tools/answer-citations/tests/api.rs`, `tools/answer-citations/tests/cli.rs`
- Fixture: `tools/answer-citations/tests/fixtures/desk-explicit.audit.jsonl`
- Producer blobs: `source/blobs/query-pg-cli-v1.md`, `source/blobs/query-pg.rs`
- Baseline: `linux/smoke/`
- Failed proofs, kept: `linux/postedit/`, `linux/postedit-attempt-2/`, `linux/postedit-attempt-3/`
- Passing proof: `linux/postedit-attempt-4/`
- Origins: `origin/recipes.txt`
- Byte inventory: `SHA256SUMS` and `product-SHA256SUMS`, written by `tools/sha256.rs`

Successful top exits are smoke 0 and postedit-attempt-4 0. Attempt 4 `test.exit` is 0 and `clippy.exit` is 0.

## Do not treat this directory as closure

`parent_acceptance` is false. This child does not mark RT-D02, SP11-W04A, or docs08 section 7 done for the parent plan. It does not judge factual entailment. It does not turn a local operator binding into an authenticated audit.

## Resources

Cleanup removes only this task's containers, network `oo-a266-eef831aa-net`, volumes `oo-a266-eef831aa-cargo` and `oo-a266-eef831aa-target`, and `/tmp/oo-a266-eef831aa-dba4-4af2-a78b-541bc9e0a266`. Exits are in `cleanup/run.log`. Pinned images stay. No prune and no foreign cleanup.

The inventory hasher uses `--network none`, 6g, and 1 CPU. `linux/inventory/launch.sh` copies its console into `linux/inventory/`. `linux/inventory/rerun.sh` rewrites the sums after that console and `cleanup/run.log` exist. The rerun console stays under `/tmp` and is removed with the task tmp directory.

## Next seam

Any broader citation rule, including numeric entailment or warning-prose review, needs its own frozen contract before code. This workspace should stay out of the root lock.
