# Handoff

Goal sign-off is open. The minimum gate on run `4458418024444808b4bf3731b53b6cb7` stands. The same binding is now generation 12, enabled, period 1000 ms, C5 reconcile. Observer pid 66510 steps only when a shadow-copy probe sees a row diff. Quiet ticks have not advanced past 12. The public gateway is stale because its catalog is still generation 6 while `max(run_generation)` is 12. Generation 12's physical table has no `oo_r_ledger` column SELECT. This source will not GRANT, will not rebind that catalog, and will not edit `onboard.py`.

Owner domain remains `tools/opensearch-preview`. Shared onboard, product flow, dialogue, gateway, pipeline, and governance sources were not edited. Customer fixture stayed read-only. No push.

## Current proof

- Run directory: `/Users/redux/.cache/oo-source-onboarding/runs/4458418024444808b4bf3731b53b6cb7` mode 0700.
- Onboard stdout: `{"state":"recipe_completed","mode":"whole_source","whole":true,"published":false}`.
- Registry: opensearch `whole` true, `whole_proof_sha256` `4a5eb3700ed5faf4b3f2d2c7689e0dd79e8b43d17bf895c048d8a4e194389f03`, recipe `1e50d79629a113b7a94220879e033f3893d2e7f0ebc02ff26526722bf33c8615`. File mode 0600. Journal identity still records the resume-time registry hash `51308e7ae6a032d4085f35f1648d4fe056e3a67fb6723d6aeef43342b84f59f9`. Do not resume this run; a later registry write changes that hash.
- `whole.py` content pin, matching the run and commit `a7909d5c4929f61e20905a3921a3db035a4f4603`: `a50e5fa2d905cc571b95a5199ba7677042b4c902c78a67ea93ba3d3532fd2a42`.

## Resources left running

- `oo-dl01w1-44-native-fresh-pg`, `oo-dl01w1-44-native-fresh-source`, `oo-opensearch-connection-api`, `oo-opensearch-connection-gateway-g5`.
- Listeners on 127.0.0.1: 18441 identity, 18443 API list, 18462 product API, 19462 gateway.
- API process `/api serve` is ELF `025aceded6cc922c0a10cb2e9d535ccc433ad6f082e5c1c3b86f4797cafc50ae`.
- Gateway process image is ELF `e253d0c0ed6283288418cb9a1566ba7b9d1f7e8011c9b0a78a24727fa80cf5dc`.
- Lease: generation 12, binding `79fc4bb0-5676-41d7-8f53-b345cbb94e55`, task `0e919da4-9fce-4e5d-8a96-b58bc3d4ac74`, enabled true, period 1000, load `awaiting_publish`.
- Observer pid 66510 runs `c5-observe.py --seconds 28800`. It is not `native-serve`. A quiet source does not enqueue a pin.
- Other sessions' processes were left alone, including shared pid 54950 (`d28ca4bd`) and PG pid 99653.
- CA and cookie material stays path-referenced under `~/.cache/oo-dl01w1-71/native-identity-private`. Nothing from those files was printed or committed.
- Git lock `/tmp/oo-dl01-wave1-git.lock` is the shared flock. Foreign staged paths were not reset.

## Retained evidence

- Failed attempts 0–4, `cancel-schema-invalid`, and the dialogue failure directories.
- Generation-5 product export at `oos-g5-retained`.
- Generation-5 gold file `gold-verdict-v2.json`.
- Pre-recovery fixture notes moved to `*.blocked-prerecovery` under `/tmp/oo-opensearch-connection-whole`.
- `CONNECTION-HANDOFF.md` in this directory is the earlier capture handoff and was not rewritten.

## Do not repeat

Recovery CAS, the paid dialogue, and the live cancel `q_JjCRmU3gOeVVrRxFkL39YtYryr`. S5 for this build is the existing file above. Do not `--resume` this run. The global registry digest is now `29ddf63d82d24b623683b980a722fa7cef8535769c8cfb920be4ea6274eaacd2`; the opensearch row is still whole. Do not rewrite the journal. Do not hand-GRANT generation 12 and do not install the generation-6 pack or catalog onto it.

## Root case

Per-table column ACL for `_oo.pt_673a7926ef6d97ac56fbe19b6fcf10ce4357a5e17f30d95ea28a926d7d20`, then a maintenance rebind whose bound generation equals `max(run_generation)`. `oo_r_ledger` has no default table SELECT. The public error after these rounds is `StaleGeneration` surfaced as `policy catalog is not available`. Shared resume drift stays with `d28ca4bd`.

## Source after current

`/tmp/oo-opensearch-connection-whole/source-after-current-20261014` Arrow `bc7a18d90b521bd71d25ea05e853819e7a74af0e8f071b7c8af2d78ad8d7bed5`, same as generation 6, reader mutations 0. Machine receipt `/tmp/oo-opensearch-connection-whole/WHOLE-RECEIPT.json` records `goal_complete` false.
