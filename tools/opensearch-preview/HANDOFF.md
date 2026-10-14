# Handoff

Goal sign-off is open. The minimum gate on run `4458418024444808b4bf3731b53b6cb7` stands. The same binding is generation 12, enabled, period 1000 ms, C5 reconcile. Observer pid 56377 steps only when a shadow-copy probe sees a row diff. Quiet ticks have not advanced past 12. The public gateway catalog is generation 12. `oo_r_ledger` has SELECT on the 14 business columns of `pt_673a7926ef6d97ac56fbe19b6fcf10ce4357a5e17f30d95ea28a926d7d20`. This source did not GRANT and did not edit `onboard.py`. Formal sign-off remains 0/10.

Owner domain remains `tools/opensearch-preview`. Shared onboard, product flow, dialogue, gateway, pipeline, and governance sources were not edited. Customer fixture stayed read-only. No push.

## Current proof

- Run directory: `/Users/redux/.cache/oo-source-onboarding/runs/4458418024444808b4bf3731b53b6cb7` mode 0700.
- Onboard stdout: `{"state":"recipe_completed","mode":"whole_source","whole":true,"published":false}`.
- Registry: opensearch `whole` true, `whole_proof_sha256` `4a5eb3700ed5faf4b3f2d2c7689e0dd79e8b43d17bf895c048d8a4e194389f03`, recipe `1e50d79629a113b7a94220879e033f3893d2e7f0ebc02ff26526722bf33c8615`. File mode 0600. Journal identity still records the resume-time registry hash `51308e7ae6a032d4085f35f1648d4fe056e3a67fb6723d6aeef43342b84f59f9`. Do not resume this run; a later registry write changes that hash.
- `whole.py` content pin, matching the run and commit `a7909d5c4929f61e20905a3921a3db035a4f4603`: `a50e5fa2d905cc571b95a5199ba7677042b4c902c78a67ea93ba3d3532fd2a42`.

## Resources left running

- `oo-dl01w1-44-native-fresh-pg`, `oo-dl01w1-44-native-fresh-source`, `oo-opensearch-connection-api`, `oo-opensearch-connection-gateway-g5`.
- Listeners on 127.0.0.1: 18441 identity, 18443 API list, 18462 product API, 19462 gateway.
- API process `/api serve` is ELF `5bc832f4a1bb9ada23a6f2a2691bf7d784d09977aa770f7a944c3c9d123be88d`, container `cf19ad67f251`. The previous file `025aceded6cc922c0a10cb2e9d535ccc433ad6f082e5c1c3b86f4797cafc50ae` remains on disk.
- Gateway process image is ELF `e253d0c0ed6283288418cb9a1566ba7b9d1f7e8011c9b0a78a24727fa80cf5dc`.
- Lease: generation 12, binding `79fc4bb0-5676-41d7-8f53-b345cbb94e55`, task `0e919da4-9fce-4e5d-8a96-b58bc3d4ac74`, enabled true, period 1000, load `awaiting_publish`.
- Observer pid 56377 runs `c5-observe.py --seconds 28800`. Pid 66510 exited earlier on a Docker socket error. The script retries that error. It is not `native-serve`. A quiet source does not enqueue a pin. Receipt snapshot: ticks 255, fires 0.
- Other sessions' processes were left alone. PG pid 99653 is not running. The column ACL receipt is the fixed installer result, not a wait on that pid.
- CA and cookie material stays path-referenced under `~/.cache/oo-dl01w1-71/native-identity-private`. Nothing from those files was printed or committed.
- Git lock `/tmp/oo-dl01-wave1-git.lock` is the shared flock. Foreign staged paths were not reset.

## Retained evidence

- Failed attempts 0–4, `cancel-schema-invalid`, and the dialogue failure directories.
- Generation-5 product export at `oos-g5-retained`.
- Generation-5 gold file `gold-verdict-v2.json`.
- Pre-recovery fixture notes moved to `*.blocked-prerecovery` under `/tmp/oo-opensearch-connection-whole`.
- `CONNECTION-HANDOFF.md` in this directory is the earlier capture handoff and was not rewritten.

## Do not repeat

Recovery CAS, S5 for build `b_e9653e459c33efde8b5ddd8cacd44365`, the earlier verified dialogue, and cancel `q_JjCRmU3gOeVVrRxFkL39YtYryr`. Do not `--resume` run 445. The global registry digest recorded in the receipt is `29ddf63d82d24b623683b980a722fa7cef8535769c8cfb920be4ea6274eaacd2`; the opensearch row was still whole. Do not rewrite the journal. Do not hand-GRANT. Do not present the generation-6 catalog as generation 12.

## Current public proof

- Gold `q_J2xS9Y546NOQVNqWIg4qkpLzZv`, 70 cells, 0 differences, verdict SHA-256 `b9a73e3f02efb7fc2e3385e0f3684fac3b599fa20a6a6f36667417ac02723f9a`. `STALE_DATA` is not a certificate.
- Dialogue verdict SHA-256 `a06fcc97e4c5a383659fffc484803610fe6b35ad3bde48af5b597e916c80011c`, transcript `5a587d6d10379440f2fa98199b0f9662afc6b011533fd9da0fea2f2e3bf37495`, queries `q_WmJg2c15AP4hrifaxSGi4jgz2A`, `q_cMhrZwR4R4t72sXrUbmA51KBtp`, `q_tvPHkytegb3Rb36bU1vybRLDIy`.
- Cancel `q_LL4oGbozXAKiNlMLQcXUsTKqnv`, sqlstate 57014, rollback true, verdict SHA-256 `c7320e4d7e01adb82f9960ec4ffedfcf7719f237f28f71271afa52173fdfa9ed`.
- Failures kept: `dialogue-g12` model timeout, `dialogue-g12b` missing knowledge tools, the two smoke commands that never reached the crate tests.
- Maintenance `llm_calls` 0. Publication `not_published`. Formal sign-off 0/10.

## Root case

The per-table column ACL is installed. The generation-12 maintenance artifact is the running gateway catalog. Shared resume drift stays with `d28ca4bd`. A single-catalog schemaDigest 409, if it appears, stays with the Oracle owner of that case. This collection maintenance returned 200.

## Source after current

`/tmp/oo-opensearch-connection-whole/source-after-current-20261014` Arrow `bc7a18d90b521bd71d25ea05e853819e7a74af0e8f071b7c8af2d78ad8d7bed5`, same as generation 6, reader mutations 0. Machine receipt `/tmp/oo-opensearch-connection-whole/WHOLE-RECEIPT.json` records `goal_complete` false.
