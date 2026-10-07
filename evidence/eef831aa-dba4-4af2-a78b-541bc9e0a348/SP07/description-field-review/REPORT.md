# Column field producer review

Verdict: **APPROVE_LIMITED** for the column-field producer seam at `c23db28dc27f5cf20741dd36e8a8e88129ad1715`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a348`. This directory is the independent review record. Parent `93f5b8aea78d0d745334da0bc5352afdb86412e2`. Tree `c959efb16348a48f682a4444a6366e44bf70c0b1`. Commit message `description-candidate: admit column text on prompt v2`, dated 2026-10-08 01:55:47 +0800.

This review does not accept E47. It does not accept SP07. It does not accept the parent goal. It does not add a countable unit. It does not edit a product file, a lock, a root manifest, a spec, or the ledger. Actual Builder integration is still unverified. Until that integration is independently verified, the whole of E47 and SP07 stays open.

The source was a git archive of `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. Worktree HEAD when proof-3 started was `69e9cfe8be8ff44ac1adebf3cb91152c4c0bf942`. That HEAD was not the build source. `tools/pg-builder` was not mounted and was not edited. a328 may continue on its own paths.

## Decisive run

`linux/proof-3`. Container `oo-a348-proof3`, id `989aea8200e0d53f3cdb177bc86101a7415abf99feebe118fd3cb31b77757d25`. `docker-run.exit` is 0. Console ends `PROOF_OK`. Launch removed the container after writing `hostconfig.txt`.

HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a348-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, ReadonlyRootfs false. `capacity.txt` before start: `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/proof-3/image.txt` `id=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 os=linux arch=arm64`. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0` `(85eff7c80 2026-01-15)`. Kernel `Linux 989aea8200e0 6.12.76-linuxkit #1 SMP Fri Apr 17 14:56:37 UTC 2026 aarch64 GNU/Linux`. PostgreSQL was not started. `cargo-update-invoked.txt` is `no`.

Phase exits, each 0: toolchain, overlay-absent, archive-locks, test-candidate, test-candidate-count, test-candidate-cli-count, test-lib, test-lib-count, test-lib-filter, test-review, test-review-count, test-pack, test-pack-count, test-pack-clean, test-candidate-bin, test-provider, test-provider-count, oracle-copy, test-a348, test-a348-count, clippy-component, clippy-builder, clippy-review, clippy-pack, clippy-provider, lock-compile, lock-subset, lock-summary, oracle-removed, product-bytes, root-lock.

Author counts, recorded before `tools/description-pack/tests/a348_sealed.rs` existed:

- `description_candidate` 10 passed
- `description_candidate_cli` 5 passed
- lib filter `description_candidate` 2 passed, 33 filtered, including `prompt_version_is_the_sealed_constant`
- `description-review` 21 passed
- `description-pack` 21 passed, and the pack log does not name `a348_sealed`
- `description-provider` in-file 3 passed, integration 23 passed

Independent oracle `linux/proof-3/test-a348.log`: `ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.39s`. Names: `explicit_column_review_reseals_two_roles_without_forging_review`, `occupied_cas_stale_defer_and_unknown_pii_fail_closed`, `v1_missing_extra_and_failures_write_nothing`. Oracle content SHA-256 `515edffdea475fd38fa2e110e3c00b142dfb3813c589b8547340ad1131d68c63`.

Clippy `-D warnings` exited 0 for oo-builder (`--lib --bin description-candidate --test description_candidate --test description_candidate_cli`), description-review `--all-targets`, description-pack `--all-targets` while the oracle was present, and description-provider `--all-targets`. All four used `--locked --offline`.

`linux/proof-3/lock-subset.stdout` ends `summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true`. Path packages are description-pack, description-review, oo-builder, oo-core, and oo-jobs. `sha2 0.10.9` is in that stdout. Root lock content SHA-256 before and after is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. Git blob `45d24ef0a15b2db24fc0b9dd02826cd1056ecae8`. Child locks match `source/blobs.txt`. `product-diff.txt` is empty. The oracle file was removed before that compare.

## What the oracle held

The fixture is local. Tables are `public.lane` and `public.bay`. An extra curated dataset is `public.quay`. Roles are `oo_r_north` / `north_user` and `oo_r_south` / `south_user`. Reviewer label `nia.declared` is distinct from the curated reviewer `ken.curator`. No database was opened and no model was called.

v1 `prepare_description_request` succeeds. Its prompt SHA-256 differs from `ec5355303ad18ce189e9a297677a55f9f6317e1ab59d553b057d05c81f6278c2`. A complete v1 response omits `columns` and emits outcome protocol `oo.builder.description-candidate/v1`. A v1 response that adds `columns` is `format_rejected` / `unknown_key`, with empty candidates and no `columns` key. A v1 candidates outcome plus a column decision is `oo.builder.description-review-command/v1/decision.unknown` and writes nothing.

v2 prompt version `oo.builder.description-prompt/v2` selects that prompt hash. The outcome protocol is `oo.builder.description-candidate/v2`. Five physical columns are returned. The crate column carries the request-card `card_sha256`, `attnum` 2, `ident` `crate_count`, and `effect` `review_only`. The card hash is not the evidence hash. The compiler column has no `pii` and no `review`. Statements are `pii_classification` `not_performed`, `auto_accept` `not_authorized`, and `confidence_effect` `recorded_only`.

A missing column, an extra column, and a wrong `attnum` are `incomplete`, with both arrays empty. The pack outcome is `not_candidates`, with no reseal and no packs. A wrong card hash is `format_rejected` / `evidence_identity`. A `pii` key on the model column is `format_rejected` / `unknown_key`, and the canary `a348_CLASS_DO_NOT_ADMIT` is absent from the outcome. `provider_failure` carries `columns: []` and `candidates: []`, and the pack does not reseal. The original wrapper hash is unchanged after each refusal.

Approve writes `label` and `description` only. `authenticated` stays false. `confidence_effect` stays `recorded_only`. Confidence `0.25`, the caveat, and the open question stay on the review queue and are absent from the pack JSON. The written crate column keeps `pii` `personal`, `status` `deprecated`, classification `confirmed`, and the original review object `{"at":"2026-10-01","by":"ken.curator","state":"approved"}`. Dataset caveats and the dataset label stay. `public.quay` stays `changed` false. Badge receives label and description, keeps `pii` `restricted`, and gains no review object.

The pack reseals through the captured-catalog compiler. Outcome `packed`. Statements include `reseal` `compiler`, `llm` `not_used`, and `source_artifact` `not_mutated`. The original wrapper still recompiles to its recorded hash. The resealed wrapper matches a fresh `compile_captured_pg_catalog`. Both role digests match a fresh `compile_role_pack` of the canonical effective document plus that role, and the two digests differ.

Without `column_descriptions`, north renders the crate label and omits the crate body, `UNREVIEWED:`, `badge_code`, the badge body, the bay body, and `private_mark`. South renders both visible labels and omits both bodies. The effective crate description is the approved body, its `review` is null, and its `pii` is `none` from the caller decision. Effective badge `pii` is `personal` from the caller decision. The curated review object is unchanged.

With `column_descriptions` and `dataset_descriptions`, north still omits the crate body because that prose names the hidden ident `badge_code`. South contains `UNREVIEWED: ` plus the crate body and `UNREVIEWED: ` plus the badge body, contains `badge_code`, contains `Samples are omitted because pii is personal.`, and omits the bay body and `private_mark`. Confidence, the caveat, the question, and the classification canary stay out of the pack text.

The token `descriptions` is `include_unreviewed.unsupported`. The message contains `was not treated as a grant`. The duplicate token `column_descriptions` is `include_unreviewed.duplicate`. Neither error contains the column bodies. The original wrapper is unchanged.

Occupied column CAS: the first reseal writes `column/public.lane/crate_count` with action `override` and exactly the keys `action`, `fact_fingerprint`, `item_id`, `schema_hash`. The reviewer name is absent from that object. A second approval with the flag absent is `unsupported` at `review.column_decision_occupied`, with no reseal. A wrong hash and an extra key are `review.occupied_cas`. A hash with the flag false is `schema.invalid` and the message `a replacement hash requires authorization`. The exact prior canonical hash reseals. `occupied_replacements` attribution is `bridge_provenance`. `prior_review_sha256` and `compiler_review_sha256` equal the prior hash. `authenticated` is false. `reviewer_label` is `nia.declared`. Fields are `description` and `label`. The four-field review bytes stay the prior object. The curated review object stays the original approved object. The label becomes the second label.

A stale column `schema_hash` returns `review.schema` and does not contain the body. Defer and reject produce `reviewed_unchanged`, a null resealed artifact, and `reseal` `not_required`. The curated description stays `kept crate note` and the review object is unchanged. A non-empty CAS map on that defer command is `review.occupied_cas`. A role that allowlists `private_mark` with no classification decision is `classification.unknown_pii`, and the error does not contain the bay body or the crate body.

## include_unreviewed

`docs/06-ontology-layer.md` prose at the merge rule names `publish.include_unreviewed: [descriptions]` and says unreviewed text is marked 未审核. The yaml example in the same section lists `column_descriptions`. REQ-OOS-05 says unreviewed descriptions enter a pack only with an explicit `include_unreviewed` and a visible mark, and that this does not widen classification or grants.

The compiler tokens are `column_descriptions`, `dataset_descriptions`, `caveats`, and `relationships`. The prose token `descriptions` is refused. It is not a silent grant. The new column description is published only when `column_descriptions` is present, and the role artifact prefixes it with `UNREVIEWED:`. A curated `review.state` of `approved` remains on the curated document and does not publish the new body, because the effective column `review` stays null. This seam does not forge `column.review` and does not copy model confidence into classification.

## Earlier attempts, kept

`linux/proof-1`, container `f9a1aeef481f0adbed0086086da4257c6f43b64502e2c45192ebbea7f84318b9`, `docker-run.exit` 101. Author counts matched the list above. The oracle failed to compile: `E0382` borrowed `outcome_text` after moving it into `World`. No product edit.

`linux/proof-2`, container `800d590a85b2aa5bb0f872767bf51e63e0395a5f3f9516db96589b50ccc62ab9`, `docker-run.exit` 101. The same author gates passed. The three oracle tests passed. description-pack Clippy rejected unused `draft_dataset`. Builder and review Clippy passed. Provider Clippy and the lock subset did not run. No product edit.

Neither directory was overwritten. Proof-3 reused the private cargo and target volumes, then those volumes were removed. See `CLEANUP.txt`.

## Limits

The approval is the behavior exercised by proof-3 on the fixed archive, plus the author tests that ran before the oracle existed. This run started no PostgreSQL, called no model, and did not exercise a live grant, an authenticated principal, or publication. `include_unreviewed` `column_descriptions` is the only publication path for the new description, and the compiler marks that text `UNREVIEWED`. A curated column review is not a grant. Builder integration of this seam is outside this verdict.
