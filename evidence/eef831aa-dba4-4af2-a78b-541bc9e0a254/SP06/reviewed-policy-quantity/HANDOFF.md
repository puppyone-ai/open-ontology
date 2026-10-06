# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a254`. Fixed commit `c828e2781fc37eb62249fab54fa0b7fff86f0249`.

## Commits on the owned product paths

- `6c426aa5839fbebaf67b84d07cfd82b8e01391af` `gateway: freeze reviewed integer quantity policy contract`
- `83b4f94a12c527ff8fd77eadd2909437e63d908c` `gateway: admit integer quantity on reviewed int2 int4 int8`
- `e447249ab1a7f4c82df4729090b1285c9ef4a4ec` `gateway: test reviewed integer quantity through the policy CLI`

The evidence commit, when present, is only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a254/`. Specs, prompts, and other sessions' evidence stay unstaged. Nothing was pushed.

## Where to read the proof

- Contract: `contracts/gateway/reviewed-policy-quantity-v1.md`
- Adapter cell: `compatible` in `crates/oo-gateway/src/reviewed_policy.rs`
- CLI fixture: `crates/oo-gateway/tests/reviewed_policy_quantity.rs`
- Baseline and retries: `linux/smoke/`, `linux/smoke-attempt-1/`, `linux/postedit/`, `linux/postedit-attempt-1/` through `postedit-attempt-3/`
- Composition: `linux/chain/`, `live/outcome.json`, `live/steps/reviewed-policy-clerk.stdout`, `live/steps/query-clerk-order_doc.stdout`
- Generation checks: `src/pg.rs` `compare_import`, `src/policies.rs` `check_policy`
- Reviews: `reviews/01-adapter-cell.md` through `reviews/04-scope.md`
- Byte inventory: `SHA256SUMS` and `SHA256SUMS.sha256` from `tools/sha256.rs`

Successful top exits are smoke 0, postedit 0, chain 0. Clerk amount cells are the int8 extremes and zero. Source generation 48. Target generation 1. Policy generation 48.

## Do not treat this directory as closure

`parent_acceptance` is false. `m1_i01` and `m1_i02` are `not_accepted`. `component_approval` is false. `development_unit` is false. a248's earlier adapter limit stays a historical record of the refusal this child amends. The parent contract files were not rewritten.

## Next seams, each frozen before any further code

Money with a unit. Ratio, event time, date, audit time, geo, json, system. Float4, float8, numeric, and date codecs. Quantity on text or bool. Policy generation copied from the target fence.

## Resources

`cleanup/run.log` is the a254-only removal: container `oo-a254-pg`, volumes `oo-a254-eef831aa-cargo`, `oo-a254-eef831aa-pgdata`, `oo-a254-eef831aa-target`, network `oo-a254-eef831aa-net`, and `/tmp/oo-a254-eef831aa-dba4-4af2-a78b-541bc9e0a254`. Hash containers `oo-a254-hash` and `oo-a254-hash-2` are removed by their launchers. Pinned images stay. No prune and no foreign cleanup.

The inventory hasher uses `--network none`. The proof network is removed before the hash. `rustc --edition 2021` compiles `tools/sha256.rs` in the pinned 1.94 image. `linux/inventory/launch.sh` copies its console and hostconfig into `linux/inventory/`. `linux/inventory/rerun.sh` rewrites `SHA256SUMS` after those files exist. Its stdout and hostconfig stay in a private tmp directory and are not copied into this tree. That tmp directory is removed after the rerun exits.

Development executions inside the cap of 12: contract, product edit, test, smoke attempt 1, smoke success, three post-edit attempts, post-edit success, chain, inventory. Four reviews are the files under `reviews/` and did not launch a builder.
