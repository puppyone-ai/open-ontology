# description-pack development record

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a292`. Frozen child S5-D05 / SP07-W03/SP08-W01. Implementation commit `5ce8b1c4186eb80f9b876a306ae94309f10933ae`. Fixed input archive `c8588faab992af1193ff58dbc489a0b117796aa0`, tree `f63d5ce2dcfbc8de759510280aa3761acf0b3e15`. Repo HEAD at the proof-3 launch was `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. This file does not accept S5-D05 for the parent, does not accept publication, does not close SP07 or SP08, and does not certify a release.

`tools/description-pack` is a standalone workspace. It is not a member of the root workspace. It calls `apply_description_review`, `compile_captured_pg_catalog`, `compile_reviewed_effective`, and `compile_role_pack`. It does not open a database, grant, authenticate, call a model, or publish.

## Decisive run

`linux/proof-3` is one container, `oo-a292-proof3`, id `f637a03cd7a7b8d6648e40dd3e8007b5df160a17e79052194202c2ced2287de5`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a292-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`. `capacity.txt` recorded `own=0 global=1 host_cargo=0 cap_global=10 cap_own=1 postgres=0` before start. The one other rust-builder was `oo-a296-overlay-2`. `containers-before.txt` also lists `oo-a297-pg` and `oo-a295-pg`. This unit did not start those containers and did not join their networks. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0` `(85eff7c80 2026-01-15)`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

`docker-run.exit` is 0. `version.exit` 0. `overlay-absent.exit` 0 before the copy. `lock-summary.exit` 0 with path packages 4 on the sealed `description-review` lock. `fetch-offline.exit` 0 and `fetch.exit` 0 on the cargo volume filled by `linux/proof-1`. `smoke-test.exit` 0, `smoke-count.txt` 16, `smoke-names.exit` 0, before `tools/description-pack` was copied. `overlay-cmp.exit` 0. `overlay-present.exit` 0: the archive mount did not gain `description-pack`. `pack-lock-summary.exit` 0 with `registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true`. `fetch-pack.exit` 0. `test.exit` 0. `clippy-component.exit` 0. `clippy.exit` 0. Clippy stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 1.12s`. `product-unchanged.exit` 0. `cargo-update-invoked.txt` is `no`. `python-present.flag` is 1. The scripts did not invoke Python. PostgreSQL was not started.

## Preserved attempts

`linux/proof-1`, container `oo-a292-proof1`, id `4dfa1c5a1d253b6c7e48465e6fe977af4e0a7e5731978375b78d8127bdf62aa9`. ExitCode 101. OOMKilled false. Author smoke exited 0 with 16 passed. Offline fetch of `description-review` exited 101 on the cold volume. Online `cargo fetch --locked` exited 0. The pack lock subset matched path count 5. `cargo test --test pack` did not compile: `tests/pack.rs:838` `error[E0277]` `char: LowerHex` on `format!("\\u{ch:04x}")`. Clippy exited 101 on that same error. `product-unchanged.exit` 0. `failure-note.txt` records it.

`linux/proof-2`, container `oo-a292-proof2`, id `24348f1ff488db6cd297c000505d634b4114b5fd67a295ee89740766673ecdbc`. ExitCode 101. OOMKilled false. Author smoke exited 0 with 16 passed. Offline fetch exited 0. Clippy exited 0. `cargo test --test pack` was 4 passed and 4 failed. The four failures returned `oo.builder.role-pack/v1/identity.invalid` because the principals were `desk.user` and `pay.user`. `check_slug` rejects `.`. `failure-note.txt` records it. `product-unchanged.exit` 0.

## Author smoke, before the overlay

`cargo test --manifest-path tools/description-review/Cargo.toml --locked --offline -- --test-threads=1` on the sealed archive. `linux/proof-3/smoke-test.stdout` sums to 16 passed. The 16 names are the fixed-commit description-review tests: `field_literals_are_independent_of_the_library_helper`, `approve_one_dataset_and_reject_the_other`, `defer_and_a_missing_decision_leave_curated_text`, `cas_mismatch_does_not_write_and_a_quoted_literal_does`, `stale_schema_and_evidence_block_every_dataset`, `one_stale_anchor_does_not_block_the_other_dataset`, `a_missing_curated_file_is_not_invented`, `decision_schema_and_candidate_hash_block_one_dataset`, `provider_failure_does_not_copy_the_captured_body`, `wrong_outcome_hash_and_a_compiler_error_return_no_document`, `unknown_keys_do_not_echo_an_injected_token`, `exact_core_bounds_apply_and_a_longer_label_is_not_truncated`, `a_backtick_description_is_refused_without_a_partial_write`, `reviewer_length_is_a_command_error_and_256_is_kept`, `duplicate_unknown_and_identity_fail_closed`, `cli_apply_matches_the_library_and_usage_stays_on_stderr`.

`linux/proof-1` and `linux/proof-2` recorded the same 16 before their pack failures.

## Pack tests

`cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test pack -- --test-threads=1`. `linux/proof-3/test.stdout`: `ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.49s`.

Names: `approved_text_is_resealed_and_packed_per_role`, `cli_matches_the_library`, `incomplete_compiler_does_not_leak_candidate_text`, `occupied_dataset_review_is_unsupported`, `reject_and_defer_leave_the_original_seal`, `review_hash_and_artifact_mismatch_refuse`, `stale_schema_and_cas_refuse_without_a_document`, `unclassified_allowlist_is_unknown_pii`.

The fixture is two catalog tables, `public.ledger` and `public.orders`, plus an extra curated dataset `public.aside`. Source `a292_src`, database `a292db`, captured at `2026-03-20T08:15:00.000000Z`. Roles `oo_r_desk` and `oo_r_pay`. Principals `desk_user` and `pay_user`. The orders description names `vault_token`. The ledger candidate label is `LEDGER_CANDIDATE_DO_NOT_APPLY`.

Results that passed:

- Approve orders label, description, and grain. Reject ledger. The original wrapper hash is unchanged after a second compile of the supplied artifact. The new wrapper hash differs. `outcome` is `packed`. `statements.reseal` is `compiler`. `source_artifact` is `not_mutated`. `database` is `not_used`. `grants` is `not_authorized`. `llm` is `not_used`. `publication` is `unavailable`.
- Curated documents keep the aside file, the ledger file, and the orders extras: entity `parcel`, row count 42, tier `raw`, caveat, column `parcel_count` pii `personal`, status `deprecated`, classification `confirmed`. The approved label, description, and grain are on the orders document.
- The resealed draft curated layer keeps column pii `sensitive`, semantic type `free_text`, and status `draft`. The draft effective layer has the approved text and column pii null. The ledger effective label is not the rejected candidate.
- An independent `compile_reviewed_effective` of the resealed artifact, the submitted decisions, and the submitted settings matches `effective_sha256`. `parcel_count` pii is `restricted` and semantic type is `quantity`.
- Independent `compile_role_pack` matches the pack digest, manifest hash, cache key, and artifact hash for each role. The desk pack contains the label and grain and does not contain `vault_token`, the description, `secret_note`, or the ledger canaries. The pay pack contains the description, `vault_token`, and `UNREVIEWED`, and does not contain `secret_note` or the ledger canaries.
- Defer orders and reject ledger. Outcome `reviewed_unchanged`. Resealed artifact is null. Curated documents are unchanged. The desk pack does not contain the approved canaries.
- A wrong decision schema hash returns `review.schema`. A wrong label `current_sha256` returns `review.cas`. The error text does not contain the canaries.
- A wrong `expected_review_sha256` returns `review.hash`. An artifact protocol change returns `seal.artifact`.
- A provider-failure captured body returns `not_candidates`. The document has no packs, curated documents, or resealed artifact. The queue reason is `provider_failure`. The body `INCOMPLETE_BODY_a292_DO_NOT_LEAK` is absent.
- A sealed review that already occupies `dataset/public.orders` returns outcome `unsupported`, seam `review.dataset_decision_occupied`, with no packs and no resealed artifact. The original wrapper hash is unchanged.
- Allowlisting unclassified `secret_note` returns `classification.unknown_pii`. The message does not contain `secret_note` or the description.
- `description-pack apply` stdout equals the library bytes, with no trailing newline, exit 0. Usage is exit 2. A bad review hash is exit 1. Stderr starts with `description-pack:` and contains `/review.hash`. An env canary is absent from the output.

## Registry triples

Pre-overlay `linux/proof-3/lock-subset.stdout` uses the unmodified a288 lock subset and ends at path packages 4, registry 117, missing 0, mismatch 0. Post-overlay `pack-lock-subset.stdout` ends with `summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true`. Path packages are `description-pack 0.1.0`, `description-review 0.1.0`, `oo-builder 0.1.0`, `oo-core 0.1.0`, and `oo-jobs 0.1.0`. Direct pins present include serde `1.0.229`, serde_core `1.0.229`, serde_derive `1.0.229`, serde_json `1.0.151`, and sha2 `0.10.9`. `cargo update` was not invoked. No lock was generated. The child lock was hand-spliced from `tools/description-review/Cargo.lock` by inserting the `description-pack` path package. `cargo fetch --locked` and `cargo test --locked` accepted it.

Child `Cargo.lock` content SHA-256 `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d`. Root `Cargo.lock` content SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Git blob of that root lock at the fixed commit is `2990f901ea98a7b465f462cce8742bc5c51c13aa`, the same blob as `216e6df82a61d2bc22548ce1a5075d5f4a045648`. The root lock and `tools/description-review` were not edited. `src-before.txt` matches `src-after.txt`.

## Archive

`linux/proof-3/archive-blobs.txt`: `crates/oo-builder/src/description_candidate.rs` want and got `f783ae95f1f7398da18b6c47bcc87941db49f7a3`. `tools/description-review/src/lib.rs` want and got `12202a5f4136b37f67786ec1ee42598bd22482b7`. Content SHA-256 of `description_candidate.rs` in the container is `c392dd9ef7dfff45ea9f6298b257e8fc4bdeca04784acbfbb696ef644f884954`. The archive has no `tools/description-pack`. `rust-toolchain.toml` was moved aside. `RUSTUP_TOOLCHAIN=1.94.0`.

## Product bytes at proof-3

These content hashes are `linux/proof-3/pack-source.sha256`. `source/blobs` matches the workspace and that snapshot. Implementation commit `5ce8b1c4186eb80f9b876a306ae94309f10933ae`.

| file | sha256 |
| --- | --- |
| `CONTRACT.md` | `cf970f86c98282c7b5bdc2a4325339a556965cddb27622e26b0ffcc77fc98592` |
| `Cargo.lock` | `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d` |
| `Cargo.toml` | `85e75020190ce8e56b32756a14c93229e44a34b085db3c960ab6012db5cf212f` |
| `src/apply.rs` | `47c1de40c495e24238562c00d733125cb1e674d5f0818f89670f3d01cef1d225` |
| `src/canonical.rs` | `f43725316fa797c7a3d7eeb0f2c539ddb380a0dd2b7400053a8dd32e94702d6a` |
| `src/error.rs` | `eac4d0679e7920a72f619a093eb3938445485fb8a683921bf4eb1fea8ff1f9d1` |
| `src/lib.rs` | `d7782a147eea84a4b8f6ba38ce10361206758673db4f5fe8979d29fa447cf6a8` |
| `src/main.rs` | `3c85f262a804d13db58f41a01246f95412592830c181d8dfd319d457061cd999` |
| `tests/pack.rs` | `819d5b89bbffe79f79b1285df6f35581459132a7755920e79875547fce1ae02f` |

## Contract order

`CONTRACT.md` was written after the first source files and before any successful compile. The requested order was contract before implementation. That order was not kept. The contract text matches the behavior exercised by `linux/proof-3`.

## Recipes

Origins and the path-count change are in `origin/recipes.txt`. The launch shape is a288 `linux/proof-4/launch.sh`, content SHA-256 `0cf5c779d5b593619aa5b438fc97ef902cc411163d93e5e78e5e30e1734c0157`, which follows a204 `linux/overlay-3/launch.sh` at `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, blob `03c8e15c60a376ef0a757c062ebd110063a0ffe6`, content SHA-256 `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee`. The a200 PostgreSQL launch, content SHA-256 `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`, was not executed. `tools/sha256_manifest.rs` is the a199 file, content SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`.
