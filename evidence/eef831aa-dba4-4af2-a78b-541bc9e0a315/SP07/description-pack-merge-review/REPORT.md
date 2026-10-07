# description-pack occupied-review amendment review

Verdict: **APPROVE_LIMITED** for the occupied-review amendment in `tools/description-pack` at implementation `ceb4be5ebe12cf581f511cc8a676050d02465c55`, archived from fixed commit `99ac19af42059a308ba9c418815619e5a081f407`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a315`. This directory is the independent review record. The author session `eef831aa-dba4-4af2-a78b-541bc9e0a309` is not this review. No product file, root lock, spec, or ledger was edited. This file does not accept S5-D05 for the parent, does not accept publication, does not close SP07 or SP08, does not raise a denominator, and does not certify a release. It does not claim full S5 authentication or authorization.

The sealed datasets are `public.berth` and `public.hold`. An extra curated dataset is `public.wharf`. Roles are `oo_r_tide` / `tide_user` and `oo_r_keel` / `keel_user`.

Contract commit `c548c8b18c4de5d75e6f8527c71569a2c311eb11` (2026-10-06T20:22:08+08:00, "description-pack: record occupied-review replacement before code") changes only `tools/description-pack/CONTRACT.md`. It is an ancestor of implementation `ceb4be5ebe12cf581f511cc8a676050d02465c55` (2026-10-06T20:37:39+08:00, "description-pack: reseal descriptive text over an occupied dataset review"), which changes only `src/apply.rs` and `tests/pack.rs` (+829/−23). The historical contract-after-code record for the original description-pack unit stays as the parent ledger left it. This review does not rewrite that history.

## Decisive run

`linux/proof-3` is the decisive attempt. Container `oo-a315-proof3`, id `d158fb4e4010e4896f57a3b82ea212cdecd45ff7805f7e084c2755816d537d12`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a315-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, ReadonlyRootfs false. `capacity.txt` recorded `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=0` before start. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0` `(85eff7c80 2026-01-15)`. Debian 13 (trixie). Kernel line `Linux d158fb4e4010 6.12.76-linuxkit #1 SMP Fri Apr 17 14:56:37 UTC 2026 aarch64 GNU/Linux`. `repo-head.txt` is `080c07ac71f38bb83def05bd211769a454e22f1b`. The archive is the fixed commit, not that HEAD.

`docker-run.exit` is 0. `version.exit` 0, `rustc-release.txt` `1.94.0`, `blob-cmp.exit` 0, `overlay-absent.exit` 0, `lock.exit` `present`, `lock-content.exit` 0, `lock-compile.exit` 0, `lock-subset.exit` 0, `lock-summary.exit` 0. `smoke-test.exit` 0, `smoke-count.txt` 14, `smoke-names.exit` 0. `clippy-component.exit` 0. `clippy-pre.exit` 0. `a302-overlay.exit` 0, `overlay-present.exit` 0. `a302-test.exit` 0, `a302-count.txt` 7, `a302-names.exit` 0. `a315-test.exit` 0, `a315-count.txt` 5, `a315-names.exit` 0. `test.exit` 0. `clippy.exit` 0. `product-unchanged.exit` 0. `cargo-update-invoked.txt` is `no`. `python-present.flag` is 1. The scripts did not invoke Python. PostgreSQL was not started.

Proof-3 reused the private cargo and target volumes from proof-1, so `fetch-offline.exit` is 0 and the Clippy times are incremental (`clippy-pre.stderr` ends `Finished dev profile [unoptimized + debuginfo] target(s) in 1.05s`; `clippy.stderr` ends in `0.22s`). Both Clippy stdout files are empty. The cold fetch is `linux/proof-1/fetch-offline.exit` 101, then online `cargo fetch --locked` exit 0.

The launch removed `oo-a315-proof3` after writing `hostconfig.txt`. `container-removed.txt` is that name.

## Earlier attempts, kept

`linux/proof-1`, container `c46dc4d3e6c3ef330aa7d2fa1032e91e7675626f358f4256c36d3f39ff762165`, `docker-run.exit` 101. Cold offline fetch 101, online fetch 0, author smoke 14, pre-overlay Clippy 0, unchanged a302 boundary 7, product bytes unchanged. All 5 a315 tests panicked in the bound compile: `curated.invalid` / `curated status must stay draft or rejected`. The oracle had put relationship status `deprecated` on the compiler curated overlay. That status is legal on an OOS dataset document and illegal on this overlay.

`linux/proof-2`, container `beb1b2c2692049b88272eca672a327bcd0fbde53f1390f8a72528a6271f5a173`, `docker-run.exit` 101. The same pre-overlay gates passed. a315 passed 3 (`accept_and_stale_fail_closed`, `flag_absent_and_cas_refusals_keep_the_prior_seal`, `reject_and_defer_leave_structural_fields`) and failed 2. `occupied_override_reseals_approved_fields_only` and `mixed_fresh_and_occupied_approvals_reseal_once` panicked because the resealed effective dataset `entity` is null. A dataset override copies label, description, grain, and source name. It does not copy `entity`. The curated echo still carries `entity`. The oracle was asserting the effective field.

Neither attempt was overwritten. Product bytes were not changed between them.

## Author smoke, before the overlay

`cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test pack -- --test-threads=1` on the sealed archive, with both overlay files absent. `linux/proof-3/smoke-test.stdout`: `ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.93s`. The stdout does not contain `a302_sealed` or `a315_sealed`.

The 14 names: `approved_text_is_resealed_and_packed_per_role`, `cli_matches_the_library`, `incomplete_compiler_does_not_leak_candidate_text`, `occupied_dataset_review_is_unsupported`, `reject_and_defer_leave_the_original_seal`, `review_hash_and_artifact_mismatch_refuse`, `stale_schema_and_cas_refuse_without_a_document`, `unclassified_allowlist_is_unknown_pii`, `occupied_override_without_authorization_stays_unsupported`, `occupied_override_approve_reseals_two_roles`, `occupied_cas_conflict_preserves_the_seal`, `occupied_accept_is_not_rewritten`, `occupied_stale_override_is_refused`, `occupied_defer_and_reject_leave_structural_overrides`.

Pre-overlay Clippy was `cargo clippy --manifest-path tools/description-pack/Cargo.toml --locked --offline --all-targets -- -D warnings`. It exited 0 before either overlay file was copied.

## Unchanged a302 boundary

`overlay/a302_sealed.rs` was mounted read-only from `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a302/SP07/description-pack-review/overlay/`. Content SHA-256 `bf043d4cecc1a23d1ced7fdb75a4a139bcf69bffa8058858f2d33bffab18d7b1`, checked inside the container before the copy. Those tests do not set `replace_existing_dataset_review`. `linux/proof-3/a302-test.stdout`: `ok. 7 passed; 0 failed; finished in 1.54s`.

Names: `approved_text_reseals_through_the_compiler`, `compiled_cli_matches_the_library`, `hash_tuple_cas_and_stale_field_refuse`, `incomplete_body_does_not_leak`, `occupied_dataset_review_is_unsupported`, `rejected_and_deferred_leave_the_original_seal`, `unclassified_allowlist_is_unknown_pii`.

With the flag absent, an occupied dataset review is still `unsupported` at seam `review.dataset_decision_occupied`.

## Independent a315 oracle

`linux/proof-3/a315-test.stdout`: `ok. 5 passed; 0 failed; finished in 2.64s`. Content SHA-256 `a2d09ffed72062750fe58871260579cf1d322b9d58372994af118b662a49fb3d`. The fixture calls `compile_captured_pg_catalog`, `compile_reviewed_effective`, and `compile_role_pack`. It does not open a database.

Names and what they held:

- `occupied_override_reseals_approved_fields_only`. Flag true and the exact canonical SHA-256 of the prior dataset review. `description-pack apply` stdout equals `apply_description_pack`, exit 0, stderr empty. Outcome `packed`. Approved label, description, and grain are on the resealed effective dataset and on the curated document. `source_name` stays `src_berth_bound`, primary key stays `["id"]`, column `keel_count` stays label `Keel count bound` and status `draft`, and its draft effective `pii` stays null. The relationship review and the primary-key review keep their canonical bytes. The resealed review array is canonically identical to the prior array. `compiler_review_sha256` equals `prior_review_sha256`. `occupied_replacements[0].attribution` is `bridge_provenance`. `descriptive_provenance` carries `reviewer_label` `nia.declared`, `reviewer_at` `2026-10-06`, `authenticated` false, and fields `description`, `grain`, `label`. Those strings are absent from the resealed artifact. Reviewed-effective `keel_count` is `pii` `restricted`, `semantic_type` `quantity`, classification confirmed by `nia.owner`. `oo_r_tide` receives the approved label and grain and omits the description that names `bearer_token`. `oo_r_keel` receives that description. Both role digests match `compile_role_pack`. `public.wharf` is unchanged. Statements: reseal `compiler`, authentication `not_claimed`, authorization `not_claimed`, database `not_used`, grants `not_authorized`, llm `not_used`, publication `unavailable`, semantic publication `not_claimed`, source artifact `not_mutated`. The original wrapper still recompiles to the same hash.
- `flag_absent_and_cas_refusals_keep_the_prior_seal`. Flag absent: outcome `unsupported`, seam `review.dataset_decision_occupied`, no reseal, no packs, no replacement rows. Wrong hash, empty map, and an extra key: refusal `review.occupied_cas`, CLI exit 1, empty stdout, stderr contains `description-pack:` and the code, and does not contain the approved label, the reviewer, or `ENV_CANARY_a315_DO_NOT_LEAK`. A hash with the flag false: `schema.invalid` and the message `a replacement hash requires authorization`. The prior wrapper is unchanged after each case.
- `accept_and_stale_fail_closed`. An occupied dataset review whose action is `accept`, with a matching CAS and the flag set, stays `unsupported` at `review.dataset_action_unsupported`. The action is not rewritten to override. A stale occupied override (`fact_fingerprint` 64 hex `1`s) refuses `review.occupied_stale` with no document. The column override and the relationship status on the prior seal stay in place. The approved label is not applied.
- `reject_and_defer_leave_structural_fields`. Defer berth and reject hold: outcome `reviewed_unchanged`, reseal `not_required`, resealed artifact null, original documents unchanged, tide pack still contains the structural label and omits the approved text. A CAS map on that command, with no occupied approval, refuses `review.occupied_cas`.
- `mixed_fresh_and_occupied_approvals_reseal_once`. This is the one branch the author left untested. The CAS map names only `dataset/public.berth`. Hold has no prior dataset review and is approved in the same command. Outcome `packed`. Replacement rows length 1, for berth only. The resealed review array is the prior reviews plus exactly one new four-field override for `dataset/public.hold`. Every original review object is still present. Both effective datasets carry their approved text. A later `compile_captured_pg_catalog` of the resealed artifact matches the recorded wrapper, draft, and evidence-envelope hashes. A later compile of the original artifact still returns the original wrapper. CLI stdout equals the library.

The compiler review object stays four keys: `item_id`, `action`, `schema_hash`, `fact_fingerprint`. Reviewer label and reviewer time are bridge output. They are not compiler review fields.

Curated document PII on `keel_count` stays `personal` / `deprecated`, with classification by `ops.lina` and review by `ken.curator`. The draft overlay column stays `sensitive` / `free_text` / `draft`. Draft effective PII stays null. Reviewed-effective is the separate `restricted` / `quantity` decision.

## Registry triples

`linux/proof-3/lock-subset.stdout` ends with `summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true`. Path packages are `description-pack 0.1.0`, `description-review 0.1.0`, `oo-builder 0.1.0`, `oo-core 0.1.0`, and `oo-jobs 0.1.0`. Direct pins present there include serde `1.0.229`, serde_core `1.0.229`, serde_derive `1.0.229`, serde_json `1.0.151`, and sha2 `0.10.9`. Registry source is `registry+https://github.com/rust-lang/crates.io-index`. `cargo update` was not invoked. No lock was generated. The root manifest and root lock were not edited. a313 remains the root-lock writer.

Child `Cargo.lock` content SHA-256 `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d`. Root `Cargo.lock` content SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Git blob of that root lock at the fixed commit is `2990f901ea98a7b465f462cce8742bc5c51c13aa`. Child lock git blob is `028fbca1196f70ac69d90616dbc14cbca209b471`.

## Archive

Fixed commit `99ac19af42059a308ba9c418815619e5a081f407`, commit tree `6c4f5e42462a2b7b4cd270dd3216d7bfce880740`. `tools/description-pack` tree `2b747801bdfa0de16f67c193c779f3427660198f` is the same at the implementation and at the fixed commit. `linux/proof-3/archive-blobs.txt` matches the fixed commit for `description_candidate.rs` (`f783ae95f1f7398da18b6c47bcc87941db49f7a3`), `tools/description-pack/src/lib.rs` (`69930324e1a3faa6347eaffc76ed5000966dc348`), `src/apply.rs` (`88936e7141a148084521b71db585b56c465de13b`), `tests/pack.rs` (`4630186bdbcdc7234d826547de53d08dc2e2e6b0`), `CONTRACT.md` (`25f0d84c8fe2b3f5ed7797d5151d05bd0cde031b`), and `tools/description-review/src/lib.rs` (`12202a5f4136b37f67786ec1ee42598bd22482b7`).

The archive contains `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `tools/description-review`, and `tools/description-pack`. The toolchain file is moved aside and `RUSTUP_TOOLCHAIN=1.94.0` is set. The overlay files are absent from `pack-source.sha256`.

| file | sha256 |
|---|---|
| `CONTRACT.md` | `478495c774c859043135dc36cdacd4d859edf135e2bc23f903357309db82a261` |
| `Cargo.lock` | `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d` |
| `Cargo.toml` | `85e75020190ce8e56b32756a14c93229e44a34b085db3c960ab6012db5cf212f` |
| `src/apply.rs` | `3bf9224d6c8896203df07ac05c0f76817338446a7d2856a16d892527f406fef2` |
| `src/canonical.rs` | `f43725316fa797c7a3d7eeb0f2c539ddb380a0dd2b7400053a8dd32e94702d6a` |
| `src/error.rs` | `eac4d0679e7920a72f619a093eb3938445485fb8a683921bf4eb1fea8ff1f9d1` |
| `src/lib.rs` | `d7782a147eea84a4b8f6ba38ce10361206758673db4f5fe8979d29fa447cf6a8` |
| `src/main.rs` | `3c85f262a804d13db58f41a01246f95412592830c181d8dfd319d457061cd999` |
| `tests/pack.rs` | `41a0d035dbd7196330b6976ead9899f977b1d760f6035807f9832939dfb83e87` |

## Limits

The approval covers the behavior exercised by the raw exits above. Replacement is opt-in. The flag defaults absent, and the old occupied seam stays. A dataset `accept` is not rewritten into an override. A stale occupied override fails closed and discards the compile. Reject and defer do not reseal. Reviewer provenance stays on the bridge row. This review started no PostgreSQL and did not exercise a live grant, an authenticated principal, or publication.
