# description-pack occupied-review extension

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a309`. Same unit as S5-D05. Contract commit `c548c8b18c4de5d75e6f8527c71569a2c311eb11`. Implementation commit `ceb4be5ebe12cf581f511cc8a676050d02465c55`. Fixed input archive `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`, tree `7e7dd9a20deb39db87d84595a6b4cd6bc77e7f70`. Repo HEAD at the proof-1 launch was `970e53606d414b045cc1e66b7bdc3097f81e84c5`. This file does not accept S5-D05 for the parent, does not accept publication, does not close SP07 or SP08, and does not certify a release.

`tools/description-pack` remains a standalone workspace. It calls `apply_description_review`, `compile_captured_pg_catalog`, `compile_reviewed_effective`, and `compile_role_pack`. It does not open a database, grant, authenticate, call a model, or publish.

## Decisive run

`linux/proof-1` is the only attempt. Container `oo-a309-proof1`, id `00fae351c38111f800e1b608ab4bd87465525a7121b74ef995deccf355d947b0`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a309-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`. `capacity.txt` recorded `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0` before start. `containers-before.txt` lists `oo-a300-pg` and `oo-a301-pg`. This unit did not start those containers and did not join their networks. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0` `(85eff7c80 2026-01-15)`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

`docker-run.exit` is 0. `version.exit` 0. The archived pack was present (`overlay-absent.exit` is `present`). `lock-summary.exit` 0 and `pack-lock-summary.exit` 0, both `registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true`. Cold-volume `fetch-offline.exit` is 101. Online `cargo fetch --locked` `fetch.exit` is 0. After the overlay, `fetch-pack-offline.exit` is 0. `smoke-test.exit` 0, `smoke-count.txt` 8, `smoke-names.exit` 0, before the overlay copy. `overlay-cmp.exit` 0. `overlay-present.exit` 0. `test.exit` 0, `test-count.txt` 14. `clippy-component.exit` 0. `clippy.exit` 0. Clippy stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 15.83s` and contains no warning. `boundary.exit` 0, seven passed. `product-unchanged.exit` 0. `cargo-update-invoked.txt` is `no`. `python-present.flag` is 1. The scripts did not invoke Python. PostgreSQL was not started.

## Preserved attempts

No failed proof directory. `linux/proof-1` is the completed run.

## Archived pack smoke, before the overlay

`cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test pack -- --test-threads=1` on `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`. `linux/proof-1/smoke-test.stdout`: `ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.44s`.

Names: `approved_text_is_resealed_and_packed_per_role`, `cli_matches_the_library`, `incomplete_compiler_does_not_leak_candidate_text`, `occupied_dataset_review_is_unsupported`, `reject_and_defer_leave_the_original_seal`, `review_hash_and_artifact_mismatch_refuse`, `stale_schema_and_cas_refuse_without_a_document`, `unclassified_allowlist_is_unknown_pii`.

## Pack tests after the overlay

Same command on the overlaid tree. `linux/proof-1/test.stdout`: `ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.96s`.

The eight archived names still pass. The six extension names are `occupied_override_without_authorization_stays_unsupported`, `occupied_override_approve_reseals_two_roles`, `occupied_cas_conflict_preserves_the_seal`, `occupied_accept_is_not_rewritten`, `occupied_stale_override_is_refused`, and `occupied_defer_and_reject_leave_structural_overrides`.

The extension fixture compiles `public.orders` and `public.ledger` with a structural overlay: dataset label `Structural orders label`, description `Structural orders prose`, grain `one structural order`, source name `src_orders_bound`, entity `parcel`, primary key `id`, and column `parcel_count` label `Count structural`, pii `sensitive`, semantic type `free_text`, status `draft`. Four compiler reviews are bound: accept `column/public.ledger/id`, override `column/public.orders/parcel_count`, a dataset review on `dataset/public.orders`, and override `primary_key/public.orders`. Description candidates still use source `a292_src`, database `a292db`, captured at `2026-03-20T08:15:00.000000Z`, reviewer `ada.reviewer`, reviewed on `2026-10-06`, and roles `oo_r_desk` / `desk_user` and `oo_r_pay` / `pay_user`.

Results that passed:

- Flag absent. Outcome `unsupported`, seam `review.dataset_decision_occupied`. No packs, no resealed artifact, no `occupied_replacements`. The original wrapper hash is unchanged. Effective label stays `Structural orders label` and source name stays `src_orders_bound`.
- Flag set and the canonical hash of the stored dataset review matches. `description-pack apply` stdout equals the library bytes, exit 0, stderr empty. Outcome `packed`. `statements.reseal` is `compiler`. `occupied_replacements[0].attribution` is `bridge_provenance`. `prior_review` is the four-field compiler object. `prior_review_sha256` equals `compiler_review_sha256`. Descriptive provenance records `ada.reviewer`, `2026-10-06`, `authenticated` false, and fields `description`, `grain`, `label`. The resealed artifact text does not contain `ada.reviewer`, `reviewer_label`, `ada.curator`, `bridge_provenance`, `Structural orders label`, or `Structural orders prose`. Envelope reviews stay canonically equal to the original reviews. `validate_dataset` on the preserved orders document is empty. The orders document keeps entity `parcel`, row count 42, column pii `personal`, status `deprecated`, and review by `ada.curator`. Ledger and aside documents are unchanged. Effective label, description, and grain are the approved text. Effective source name stays `src_orders_bound`, primary key stays `id`, and `parcel_count` keeps label `Count structural`, status `draft`, and pii null. The curated layer keeps entity `parcel` and column pii `sensitive`. Ledger column action stays `accept`. Parcel column action stays `override`. The primary-key review hash is unchanged. An independent `compile_reviewed_effective` matches `effective_sha256`. Reviewed `parcel_count` pii is `restricted` and classification by is `ada.owner`. Independent `compile_role_pack` matches both roles. Desk omits the description and `Structural orders label`. Pay omits `ada.curator` and `Structural orders label`.
- A wrong hash, an empty map, and an extra key `dataset/public.ledger` each refuse `review.occupied_cas`. The error text omits the approved label, `ada.reviewer`, the review hash, and the structural label. Flag false with a hash refuses `schema.invalid` and the message is `a replacement hash requires authorization`. The original wrapper and the structural column status `draft` remain.
- Dataset action `accept` with a real fingerprint and a matching hash returns `unsupported`, seam `review.dataset_action_unsupported`. The action stays `accept`. The column override stays `override`. No reseal.
- Dataset override whose fingerprint is `11` repeated refuses `review.occupied_stale`. The original wrapper remains. Effective label is not the approved label. Column status stays `draft`. The stale fingerprint stays on the dataset review. The primary-key action stays `override`.
- Defer orders and reject ledger, with the flag set and no hash map, returns `reviewed_unchanged`. Reseal is null. No `occupied_replacements`. The orders curated document is the original file. The desk pack contains `Structural orders label` and omits the approved label and description. Effective description and source name stay structural. The same decisions with a hash map refuse `review.occupied_cas`. The wrapper is unchanged.

## a302 boundary oracle

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a302/SP07/description-pack-review/overlay/a302_sealed.rs`, content SHA-256 `bf043d4cecc1a23d1ced7fdb75a4a139bcf69bffa8058858f2d33bffab18d7b1`, was mounted read-only and copied into the container work tree only for `cargo test --test a302_sealed`. It was removed before `pack-source.sha256`. It is not part of `tools/description-pack`. `linux/proof-1/boundary.stdout`: `ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.66s`.

Names: `approved_text_reseals_through_the_compiler`, `compiled_cli_matches_the_library`, `hash_tuple_cas_and_stale_field_refuse`, `incomplete_body_does_not_leak`, `occupied_dataset_review_is_unsupported`, `rejected_and_deferred_leave_the_original_seal`, `unclassified_allowlist_is_unknown_pii`.

Those commands omit `replace_existing_dataset_review` and `expected_existing_review_sha256`. The occupied case stays seam `review.dataset_decision_occupied`.

## Registry triples

`linux/proof-1/lock-subset.stdout` and `pack-lock-subset.stdout` both end with `summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true`. Path packages are `description-pack 0.1.0`, `description-review 0.1.0`, `oo-builder 0.1.0`, `oo-core 0.1.0`, and `oo-jobs 0.1.0`. Direct pins present include serde `1.0.229`, serde_core `1.0.229`, serde_derive `1.0.229`, serde_json `1.0.151`, and sha2 `0.10.9`. `cargo update` was not invoked. No lock was generated.

Child `Cargo.lock` content SHA-256 `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d`. Root `Cargo.lock` content SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Git blob of the root lock is `2990f901ea98a7b465f462cce8742bc5c51c13aa`. Git blob of the child lock is `028fbca1196f70ac69d90616dbc14cbca209b471`. Both match the fixed archive. `src-before.txt` matches `src-after.txt` for the root lock, `crates/oo-builder/src/draft.rs`, `crates/oo-builder/src/captured_pg.rs`, and `tools/description-review`.

## Archive

`linux/proof-1/archive-blobs.txt` records want and got equal for `crates/oo-builder/src/draft.rs` `9505a8fd63e5c2517622c695d9ffeda4a639b562`, `crates/oo-builder/src/captured_pg.rs` `373bcad7968d88d9be331e4622b63d1ca60b1449`, `tools/description-review/src/lib.rs` `12202a5f4136b37f67786ec1ee42598bd22482b7`, archived `tools/description-pack/src/apply.rs` `bc8f84df4b794851c0e9cf51d6ca0c529ef4b1be`, archived `tests/pack.rs` `51d4e1ffe95142a211affd032ca56742f515406f`, the child lock, and the root lock. `rust-toolchain.toml` was moved aside. `RUSTUP_TOOLCHAIN=1.94.0`.

## Product bytes at proof-1

These content hashes are `linux/proof-1/pack-source.sha256`. `source/blobs` matches that snapshot and commit `ceb4be5ebe12cf581f511cc8a676050d02465c55`.

| file | sha256 |
| --- | --- |
| `CONTRACT.md` | `478495c774c859043135dc36cdacd4d859edf135e2bc23f903357309db82a261` |
| `Cargo.lock` | `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d` |
| `Cargo.toml` | `85e75020190ce8e56b32756a14c93229e44a34b085db3c960ab6012db5cf212f` |
| `src/apply.rs` | `3bf9224d6c8896203df07ac05c0f76817338446a7d2856a16d892527f406fef2` |
| `src/canonical.rs` | `f43725316fa797c7a3d7eeb0f2c539ddb380a0dd2b7400053a8dd32e94702d6a` |
| `src/error.rs` | `eac4d0679e7920a72f619a093eb3938445485fb8a683921bf4eb1fea8ff1f9d1` |
| `src/lib.rs` | `d7782a147eea84a4b8f6ba38ce10361206758673db4f5fe8979d29fa447cf6a8` |
| `src/main.rs` | `3c85f262a804d13db58f41a01246f95412592830c181d8dfd319d457061cd999` |
| `tests/pack.rs` | `41a0d035dbd7196330b6976ead9899f977b1d760f6035807f9832939dfb83e87` |

Git blobs at the implementation commit: `src/apply.rs` `88936e7141a148084521b71db585b56c465de13b`, `tests/pack.rs` `4630186bdbcdc7234d826547de53d08dc2e2e6b0`, `CONTRACT.md` `25f0d84c8fe2b3f5ed7797d5151d05bd0cde031b`.

## Contract order

`CONTRACT.md` was committed in `c548c8b18c4de5d75e6f8527c71569a2c311eb11` before `src/apply.rs` and `tests/pack.rs`. The a292 record that its own contract was written after the first source files stays in that unit's report. This extension does not rewrite that history. The default seam `review.dataset_decision_occupied` remains the result when the caller does not set the flag.

## Recipes

Origins and the archive inversion are in `origin/recipes.txt`. The launch shape is a292 `linux/proof-3/launch.sh`, content SHA-256 `04c5706acbaa1a96d905f3bb4148973f0732c90dd05711a6110ab95dc64dd9d7`, which follows a288 proof-4 content SHA-256 `0cf5c779d5b593619aa5b438fc97ef902cc411163d93e5e78e5e30e1734c0157` and a204 overlay-3 at `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, blob `03c8e15c60a376ef0a757c062ebd110063a0ffe6`, content SHA-256 `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee`. The a200 PostgreSQL launch, content SHA-256 `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`, was not executed. `tools/sha256_manifest.rs` is the a199 file, content SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. `tools/lock_subset.rs` is the a292 file, content SHA-256 `dfa03497423b1b35a427aadf6df175fd58d7355ad5c8dab7bb3364c2d31f5454`.
