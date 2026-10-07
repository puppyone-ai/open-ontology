# Findings

Verdict: **APPROVE_LIMITED**. The decisive container is `linux/proof-3`, id `d158fb4e4010e4896f57a3b82ea212cdecd45ff7805f7e084c2755816d537d12`, exit 0.

No product defect was observed in the exercised scope. The two earlier non-zero exits are oracle mistakes. Both logs are kept.

## What the amendment does

An occupied dataset review is still unsupported when `replace_existing_dataset_review` is absent or false. The seam is `review.dataset_decision_occupied`. The a302 seven-test file, byte-identical and without the flag, still ends on that seam.

When the flag is true, the command must carry `expected_existing_review_sha256` whose keys are exactly the occupied dataset item ids under the filled or replaced approvals. The hash is the SHA-256 of the canonical JSON of that one stored review. A wrong hash, a missing key, or an extra key refuses `review.occupied_cas` and writes no document. A hash without the flag refuses `schema.invalid` with the static message `a replacement hash requires authorization`, and that check happens before `apply_description_review` rewrites anything.

The action on the occupied review must be `override`. An `accept` with a matching hash stays `review.dataset_action_unsupported` and is not rewritten. A stale occupied override refuses `review.occupied_stale`, discards the compile, and leaves the prior wrapper in place. Reject and defer stay `reviewed_unchanged`. A non-empty CAS map with no occupied approval refuses `review.occupied_cas`.

A successful occupied override writes the approved label, description, and grain through a new compiler reseal, then through reviewed-effective and both role packs. Every existing compiler review object stays canonically identical on that path. `compiler_review_sha256` equals `prior_review_sha256` because those bytes did not change. Reviewer label, reviewer time, and `authenticated: false` are copied onto `occupied_replacements` as `bridge_provenance` / `descriptive_provenance`. They are not fields of the compiler review object. The resealed artifact does not contain `nia.declared`, `bridge_provenance`, or the structural label and prose.

`source_name`, primary key, column label and status, the relationship review, curated-document PII, classification, and the untouched `public.wharf` document stay put. Draft effective column PII stays null. The reviewed-effective value is a separate decision.

## Mixed fresh and occupied

`reseal` appends override reviews only for approvals whose dataset item is not already in the seal, then compiles a second time. The author suite did not call that branch. `mixed_fresh_and_occupied_approvals_reseal_once` does. The CAS map names only the occupied berth dataset. Hold is a new approval. The resealed review list is the prior list plus one four-field hold override. Berth's prior review object is still present. Replacement rows length is 1. Both wrappers recompile to the hashes the pack recorded. The original artifact's wrapper is unchanged.

## Oracle corrections, not product changes

Proof-1 used relationship status `deprecated` on the compiler curated overlay. `require_status` allows only `draft` and `rejected`. The bound compile returned `curated.invalid`. `merge_effective` also drops `status: rejected` from the effective list, so a rejected relationship would not remain as an effective status to compare. The preserved relationship status under test is `draft`, which is distinct from a deleted relationship and is legal. The description-document column status `deprecated` is a different schema and stayed `deprecated`.

Proof-2 asserted that the resealed effective dataset `entity` was `keel`. Dataset override copies label, description, grain, and source name. It does not copy `entity`. The effective entity is null. The curated echo still has `entity` `keel`. The assertion now matches that.

## Not in this verdict

This file does not accept S5-D05 for the parent, does not accept publication, does not close SP07 or SP08, does not raise a denominator, and does not certify a release. Authentication and authorization are `not_claimed`. The database was not used. Grants were not authorized. No model was called. Publication is `unavailable`. The root lock was not edited.
