# Review 3 — composition

Scope: `linux/chain/` and `live/` for this directory. Chain exit 0, container `1707f5f828c670c6953789693f0b8822ceb0ad0cf20d5beaa2e0d1ab990bfbd7`, memory 6442450944, NanoCpus 1000000000, CapDrop ALL, privileged false, network `oo-a254-eef831aa-net`. No second PostgreSQL was started. This review did not relaunch the chain.

## Checks

`live/outcome.json` has `source_generation` 48 and `target_generation` 1. `src/pg.rs` reads those columns from `_oo.full_import_run` by the import run id and reads the adopted fence from `_oo.target_generations`. The chain reached `quantity_chain=ok`, which is after `compare_import`. A mismatch there stops at `import-pg`. Policy documents in `reviewed-policy-clerk.stdout` use generation 48. `check_policy` returns an error when generation equals `target_generation`, then requires generation to equal the mapping run generation.

Clerk stdout contains `amount` with `"semantic_type":"quantity"` and pii class `none`. `install-column-acl-clerk.exit` is 0. Analyze exit is 0 and `analyze-pg.request.json` carries `source_generation` 48. Clerk order query stdout rows:

- order 701, amount int `-9223372036854775808`
- order 702, amount int `9223372036854775807`
- order 703, amount int `0`

The star query repeats those three amount cells. Peek order rows are order id, customer id, and the posted flag. That role's projection does not include amount. Int2 and int4 amounts are not columns of this imported document. They are covered by review 2.

`parent_acceptance`, `component_approval`, and `development_unit` are false. `m1_i01` and `m1_i02` are `not_accepted`.

## Verdict

The chain shows the a248 int8 quantity column passing the reviewed-policy CLI, the existing installer, analyze, and the typed query, with the source cut and the target fence kept distinct. No harness edit from this review.
