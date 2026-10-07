# Column-candidate producer

Verdict: **DELIVERED_FOR_A328**. Producer commit `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. Parent `93f5b8aea78d0d745334da0bc5352afdb86412e2`. Fixed archive `9cdbc625d02fca2fc3fd31b62ce8cb03692a9b81`, tree `a78e925e6cd8b0914702c80d9a65f76c5fc1185f`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a339`. Actor a339. This directory is the development record for the dataset-column description seam. The decisive Linux run is `linux/proof-7`. Its overlay digests are the blobs in the producer commit. `source/blobs.txt` lists them.

This packet delivers the producer API that a328 can call for S9 review and an S10 two-role pack. It does not close E47. It does not accept the parent goal. It does not add a countable unit. It does not publish, certify, or call a model. Model calls stay with a328. a328 commit `149944271ef1c2cb555954d65248413df2166f1c` is the waiting S5 consumer.

The parent commit and the fixed archive contain the same bytes on the owned product paths. Proof-7 compiled a git archive of the fixed commit, then copied the owned overlay. `linux/proof-7/repo-head.txt` is `73e63d261615111dcdefa98c25b17b1afb4d88f3`, the worktree HEAD when that container started. Later commits on master did not change these owned paths before `c23db28dc27f5cf20741dd36e8a8e88129ad1715`.

`crates/oo-builder/src/bin/description-candidate.rs` and `crates/oo-builder/src/lib.rs` match the fixed archive. v2 protocol constants stay inside `description_candidate` and `description-review`. The root manifests, root lock, and `tools/pg-builder` were not edited.

## Decisive run

`linux/proof-7`. Container `oo-a339-proof7`, id `02949443c871a986285e9a6cb82836d4cc53a05a2d3c31aa7d431f263a02e744`. `docker-run.exit` is 0. Console ends `PROOF_OK`. Launch removed the container after writing `hostconfig.txt`.

HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a339-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, ReadonlyRootfs false. `capacity.txt` before start: `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/proof-7/image.txt` `id=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 os=linux arch=arm64`. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0` `(85eff7c80 2026-01-15)`. Kernel `Linux 02949443c871 6.12.76-linuxkit #1 SMP Fri Apr 17 14:56:37 UTC 2026 aarch64 GNU/Linux`. PostgreSQL was not started. `cargo-update-invoked.txt` is `no`.

Phase exits, each 0: toolchain, overlay, test-candidate, test-lib, test-lib-filter, test-review, test-pack, test-candidate-bin, test-provider, clippy-component, clippy-builder, clippy-review, clippy-pack, clippy-provider, lock-compile, lock-subset, lock-summary, root-lock.

Test counts from the proof-7 logs:

- `description_candidate` 10 passed
- `description_candidate_cli` 5 passed
- lib filter `description_candidate` 2 passed, 33 filtered, including `prompt_version_is_the_sealed_constant` and `hidden_note_schema_hash_is_locked`
- `description-review` 21 passed
- `description-pack` 21 passed
- `description-provider` in-file 3 passed, integration 23 passed

`linux/proof-7/lock-subset.stdout` ends `summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true`. Path packages are description-pack, description-review, oo-builder, oo-core, and oo-jobs. Line 87 records `sha2 0.10.9`. Root lock content SHA-256 before and after the run is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. Git blob `45d24ef0a15b2db24fc0b9dd02826cd1056ecae8`. Child lock content SHA-256: description-review `eed8dd34b188b9fa6a22d907373cab8242b48014d6dc21878c24828c01f011dc`, description-pack `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d`, description-provider `d6b28e14bceb66512cc44acbbdce9af84754c860bef199384eb0b1662f9258cb`. The lock-subset tool content SHA-256 matches `dfa03497423b1b35a427aadf6df175fd58d7355ad5c8dab7bb3364c2d31f5454`.

## What the seam does

`binding.prompt_version` `oo.builder.description-prompt/v2` selects prompt SHA-256 `ec5355303ad18ce189e9a297677a55f9f6317e1ab59d553b057d05c81f6278c2`. The response protocol is `oo.builder.description-response/v2`. The outcome protocol is `oo.builder.description-candidate/v2` and adds `columns`. A v1 binding still emits `oo.builder.description-candidate/v1` and omits `columns`. v2 failure outcomes carry `columns: []` and `candidates: []`.

Each accepted column carries `attnum`, `card_sha256`, `caveats`, `column`, `confidence`, `dataset`, `description`, `effect` `review_only`, `ident`, `label`, and `open_questions`. `card_sha256` is the description-request card hash. `ident` is copied from that card. The physical `column` is the card name. Raw comments, unknown keys, and a partial column set produce no candidates.

Review protocol `oo.builder.description-review/v2` adds `column_decisions`. An approve writes only `label` and `description` through the typed `DatasetFile`. `authenticated` stays false. `confidence_effect` is `recorded_only`. Confidence, caveats, and open questions stay on the column decision record and on the review queue. Column `pii`, `semantic_type`, `classification`, `status`, `unit`, and column `review` are left as the curated file already had them. A v1 outcome with a non-empty `column_decisions` array is `decision.unknown`.

Pack protocol stays `oo.builder.description-pack/v1`. A filled or replaced column label or description is resealed through `compile_captured_pg_catalog`, then packed with `compile_reviewed_effective` and `compile_role_pack`. S10 classifications are the caller’s existing pack decisions. The role filter is the existing compiler. Column description text appears in a role pack when that column review is already approved, or when the command’s `include_unreviewed` lists `column_descriptions`. The shared fixture settings list `dataset_descriptions` only. The positive column role test opts that one command into `column_descriptions` as well. Labels follow the existing role filter. Hidden and personal-disallowed columns, including their text, stay out of the role pack.

Occupied dataset CAS from a315 is unchanged. Occupied columns use `replace_existing_column_review` and `expected_existing_column_review_sha256`. Flag absent is `review.column_decision_occupied`. A hash without the flag is `schema.invalid`. A wrong set or hash is `review.occupied_cas`. An action other than `override` is `review.column_action_unsupported`.

## Column tests that passed on proof-7

Review: `column_review_applies_label_and_keeps_confidence_off_the_dataset`, `column_response_failures_emit_no_candidates`, `column_stale_defer_reject_and_missing_do_not_write`, `column_interpolation_is_core_schema_and_does_not_write`, `v1_review_rejects_column_decisions`.

Pack: `column_text_is_resealed_and_role_filtered`, `column_only_approval_reseals`, `column_defer_and_reject_do_not_reseal`, `column_response_failures_do_not_reseal`, `column_stale_schema_refuses_without_a_document`, `column_allowlist_without_classification_is_unknown_pii`, `occupied_column_review_keeps_the_prior_cas`.

The existing description-candidate 10, CLI 5, review 16, and pack 14 names stayed in those totals. Hidden schema hash `49c560bd3d46e627813a8352a165fd042a1fbce1acbf7b51979ace0b97f330f2` stayed locked.

## Earlier attempts, kept

`linux/smoke` exit 1. `rm -rf /work` failed on the bind mount. `cargo fetch --locked -p` is rejected by cargo 1.94. No product edit.

`linux/smoke-2` exit 101. Candidate 10 and 5, review 16, and pack 14 passed. Provider compile missed `contracts/sdk/json-query/fixtures/tls`. No product edit.

`linux/smoke-3` exit 101. The same API, CLI, review, and pack counts passed. Provider passed 22 and failed 1. `valid_response_is_review_only_and_matches_the_cli` panicked at `tests/provider.rs:804` with `cli path: NotPresent` because `OO_A263_DESCRIPTION_CANDIDATE` was unset. That file is outside this actor’s ownership. The failure is the harness. Smoke-3 did not deliver the provider.

`linux/smoke-4` exit 0, `SMOKE_OK`. Same fixed archive, TLS fixtures present, and the env set to the compiled `description-candidate` binary. Provider 23 passed, including that CLI test. This is the pre-edit baseline. Product edits started after it.

`linux/proof-1` exit 101. `description_candidate.rs` moved `digest` and then borrowed it (`E0382`). Host fix clones `digest` before the column index uses it.

`linux/proof-2` exit 101. Candidate, lib, and review passed (review 21). Pack tests passed a `String` to `serde_json::from_str`. Host fix parses `&request_text`.

`linux/proof-3` exit 101. Pack compiled, 19 passed, 2 failed. `column_only_approval_reseals` saw effective dataset label `orders` because a column-only reseal leaves the dataset label on the draft. `column_text_is_resealed_and_role_filtered` saw the vault label and omitted the vault body because the shared settings list `dataset_descriptions` only. Host fix compares the effective label with the pre-reseal draft, and that one command adds `column_descriptions`. Column review state was not forged.

`linux/proof-4` exit 101. Focused tests passed. Clippy on oo-builder reported `too_many_arguments` (8/7) and `collapsible_if` in `outcome_value`. Host fix uses one `Listed` value and a let chain.

`linux/proof-5` exit 101. Tests and oo-builder Clippy passed. description-review Clippy reported `column_stale` at 8/7. Host fix reads `outcome.request_sha256` inside the function.

`linux/proof-6` exit 101. Tests, oo-builder Clippy, and description-review Clippy passed. description-pack Clippy reported `collapsible_if` in `queue_projection`. Host fix is one let chain for the optional `column` key. Proof-7 is that fix, and `clippy-pack.exit` is 0.

Each of those directories still has its `docker-run.exit`, logs, and `ATTEMPT.txt` where one was written. None was overwritten.

## Limits

The approval a328 can rely on is the behavior exercised by proof-7 on the producer blobs. v1 prompt, request, and outcome bytes stay on the v1 protocols when `prompt_version` is `oo.builder.description-prompt/v1`. Confidence is recorded and is not copied into an owner approval or an S10 classification. This run started no PostgreSQL, called no model, and did not exercise a live grant or publication.
