# Handoff

Owned paths: `tools/selfbuild-control-api/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a373/SP07/selfbuild-control-api/**`. No root manifest change. No push.

G1 and G2 keep the operation IDs in `INTERFACE.md`. Identity authority for a live call is a typed `getIdentityCurrent` envelope from `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. Mutations also require `Origin` on the deployment allowlist and the CSRF token from the caller's latest current, verified by the identity server before this service calls current again.

## Dependency pins

| Product | Commit | Use |
|---|---|---|
| control-identity contract | `089b00d7f70da1bc42f7e201dfa070748c651d5b` | schemas only, unchanged |
| control-identity product | `a6f92da8a0bf872f8a30efb116c2c7b14463505b` | independently accepted, private archive |
| review-ledger | `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b` | binary reused, authenticated false |
| builder pack | `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba` | binary reused |
| rules | `e5427c1a18e2ddbb480791d214c76888eb7ca998` | binary reused |
| suggestions | `d22187ea0fce8e420323660841f2da3b447d5de8` | binary reused |

## Shop result

Failure job `b_c8b4a42a5484fd235a0a9a7e9e6bd88f` remains S5 `generate.failed`. Convergence job `b_172745443da950f62320bdd1e2cec895` is `succeeded` at S10 with S0–S10 succeeded. Pack `c4f5c00c3ac9fcd640c476a3135126b486212d2317761214d1d269fdf445653c` has roles `oo_r_analyst` and `oo_r_clerk`, `published` false, `reseal_count` 1, `field_candidates_applied` true. Restart GET returned the same output hash. Second build `b_2d362e21f712095099f255347e9093c3` hit the model cache (misses 0, sockets opened 0) and stored the same pack hash. Cancel job `b_369d1e63bae4faa084ffc0c3474dd4a1` is `canceled` and its pack GET is 403. Declared cost is `unknown`. Ledger `authenticated` is false on every converge decision.

G1 and G2 operation IDs are unchanged. The fifteen operations are listed in `INTERFACE.md`.

## Reproduce

Linux image pin above, `--pull never`, `--platform linux/arm64`, 6GiB, 1 CPU. From `tools/selfbuild-control-api`:

`cargo clippy --locked --offline --all-targets -- -D warnings`

`cargo test --locked --offline --test contract -- --test-threads=1`

The shop workflow is `OO_SELFBUILD_WORKFLOW=1` and `OO_SELFBUILD_PHASE=failure` then `success` against the private config. The private script is `/tmp/oo-a373/inside-100k.sh`. It is not a repo path.

## Still open

Customer IdP, publication, harness, full parent goal, and OC-01–OC-06. `compiled_pack` is not a release and not an Agent pack. `oo-a373-pg` remains running. Volumes `oo-a373-cargo`, `oo-a373-target`, and `oo-a373-pgdata` stay. `/tmp/oo-a373` holds the private archives and is not a repo path.

This card's SB-01–SB-11 evidence is in `FINAL-AC.json`. Full SP07 is not accepted.
