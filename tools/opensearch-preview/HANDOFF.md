# Handoff

Owner domain remains `tools/opensearch-preview`. Shared onboard, product flow, dialogue, gateway, and governance sources were not edited. Customer fixture stayed read-only. No push.

## Current proof

- Run directory: `/Users/redux/.cache/oo-source-onboarding/runs/4458418024444808b4bf3731b53b6cb7` mode 0700.
- Onboard stdout: `{"state":"recipe_completed","mode":"whole_source","whole":true,"published":false}`.
- Registry: opensearch `whole` true, `whole_proof_sha256` `4a5eb3700ed5faf4b3f2d2c7689e0dd79e8b43d17bf895c048d8a4e194389f03`, recipe `1e50d79629a113b7a94220879e033f3893d2e7f0ebc02ff26526722bf33c8615`. File mode 0600. Journal identity still records the resume-time registry hash `51308e7ae6a032d4085f35f1648d4fe056e3a67fb6723d6aeef43342b84f59f9`. Do not resume this run; a later registry write changes that hash.
- Working `whole.py` pin used by the run: `a50e5fa2d905cc571b95a5199ba7677042b4c902c78a67ea93ba3d3532fd2a42`.
- Pre-commit HEAD content of `whole.py`: `db834d1aa2e7244ebc77ba4cf0ea5f29e89a2ad34d7f9e8642fa9f14a5e3487a`.

## Resources left running

- `oo-dl01w1-44-native-fresh-pg`, `oo-dl01w1-44-native-fresh-source`, `oo-opensearch-connection-api`, `oo-opensearch-connection-gateway-g5`.
- Listeners on 127.0.0.1: 18441 identity, 18443 API list, 18462 product API, 19462 gateway.
- API process `/api serve` is ELF `025aceded6cc922c0a10cb2e9d535ccc433ad6f082e5c1c3b86f4797cafc50ae`.
- Gateway process image is ELF `e253d0c0ed6283288418cb9a1566ba7b9d1f7e8011c9b0a78a24727fa80cf5dc`.
- Lease: generation 6, binding `79fc4bb0-5676-41d7-8f53-b345cbb94e55`, task `71f9b4f6-4f76-4e92-acb4-c60d155b24ea`, enabled false, load `awaiting_publish`.
- No host writer for this run was running at handoff. Other sessions' processes were left alone.
- CA and cookie material stays path-referenced under `~/.cache/oo-dl01w1-71/native-identity-private`. Nothing from those files was printed or committed.
- Git lock `/tmp/oo-dl01-wave1-git.lock` is the shared flock. Foreign staged paths were not reset.

## Retained evidence

- Failed attempts 0–4, `cancel-schema-invalid`, and the dialogue failure directories.
- Generation-5 product export at `oos-g5-retained`.
- Generation-5 gold file `gold-verdict-v2.json`.
- Pre-recovery fixture notes moved to `*.blocked-prerecovery` under `/tmp/oo-opensearch-connection-whole`.
- `CONNECTION-HANDOFF.md` in this directory is the earlier capture handoff and was not rewritten.

## Do not repeat

Recovery CAS, native-step, the live cancel, and the paid dialogue. S5 for this build is the existing file above. A new onboard input would be a different run; this watermark is already 6.
