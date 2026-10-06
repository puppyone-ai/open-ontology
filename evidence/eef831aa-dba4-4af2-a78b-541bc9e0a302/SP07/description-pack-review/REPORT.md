# description-pack independent review

Verdict: **APPROVE_LIMITED** for `tools/description-pack` at implementation `5ce8b1c4186eb80f9b876a306ae94309f10933ae`, archived from fixed commit `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a302`. This directory is the independent review record. The author session `eef831aa-dba4-4af2-a78b-541bc9e0a292` and its `linux/proof-3` are the author's run. They are not this review. No product file, root lock, spec, or ledger was edited. This file does not accept S5-D05 for the parent, does not accept publication, does not close SP07 or SP08, and does not certify a release.

The sealed datasets are `public.invoice` and `public.crate`. An extra curated dataset is `public.shelf`. Roles are `oo_r_gate` / `gate_user` and `oo_r_lane` / `lane_user`.

`CONTRACT.md` and the pack sources share one git commit, `5ce8b1c4`. The author REPORT and FINDINGS record that the contract was written after the first source files and before a successful compile. The plan ledger keeps that contract-after-code deviation and says an independent review cannot invent an earlier contract freeze. This review does not invent one. The contract text matches the behavior `linux/proof-1` exercised.

## Decisive run

`linux/proof-1` is the only attempt. Container `oo-a302-proof1`, id `a1485edc63b52902f6437dcc2788a82e7e77120c641b214349714a4df2c7548c`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a302-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`, ReadonlyRootfs false. `capacity.txt` recorded `own=0 global=1 host_cargo=0 cap_global=10 cap_own=1 postgres=0` before start. The one other rust-builder was `oo-a299-overlay-3`. This unit did not start that container and did not join its network. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0` `(85eff7c80 2026-01-15)`. Debian 13. Kernel line `Linux a1485edc63b5 6.12.76-linuxkit #1 SMP Fri Apr 17 14:56:37 UTC 2026 aarch64 GNU/Linux`. `repo-head.txt` is `72dc9e2db841de4de368216bbbe13d738f850ea9`. The archive is the fixed commit, not that HEAD.

`docker-run.exit` is 0. `version.exit` 0, `rustc-release.txt` `1.94.0`, `blob-cmp.exit` 0, `overlay-absent.exit` 0, `lock.exit` `present`, `lock-compile.exit` 0, `lock-subset.exit` 0, `lock-summary.exit` 0. `fetch-offline.exit` 101 on the cold volume (`no matching package named serde_json`). Online `cargo fetch --locked` then exited 0. `smoke-test.exit` 0 and `smoke-names.exit` 0 before Clippy and before the overlay was copied. `smoke-count.txt` is 8. `clippy-component.exit` 0 (`info: downloading component clippy`). `clippy-pre.exit` 0. `overlay-present.exit` 0, `test.exit` 0. `clippy.exit` 0. `clippy-pre.stdout` and `clippy.stdout` are empty. Clippy pre stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 16.13s`. Clippy post stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.19s`. `product-unchanged.exit` 0. `src-before.txt` matches `src-after.txt`. `cargo-update-invoked.txt` is `no`. `python-present.flag` is 1. The scripts did not invoke Python. PostgreSQL was not started. The pinned PostgreSQL image was not pulled and was not run.

The launch removed `oo-a302-proof1` after writing `hostconfig.txt`. `container-removed.txt` is that name.

## Author smoke, before the overlay

`cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test pack -- --test-threads=1` on the sealed archive, with `tests/a302_sealed.rs` absent. `linux/proof-1/smoke-test.stdout` sums to 8 passed, 0 failed, finished in 1.46s. The stderr running line is `tests/pack.rs`. The stdout does not contain `a302_sealed`.

The 8 names: `approved_text_is_resealed_and_packed_per_role`, `cli_matches_the_library`, `incomplete_compiler_does_not_leak_candidate_text`, `occupied_dataset_review_is_unsupported`, `reject_and_defer_leave_the_original_seal`, `review_hash_and_artifact_mismatch_refuse`, `stale_schema_and_cas_refuse_without_a_document`, `unclassified_allowlist_is_unknown_pii`.

Pre-overlay Clippy was `cargo clippy --manifest-path tools/description-pack/Cargo.toml --locked --offline --all-targets -- -D warnings`. It exited 0 before `overlay/a302_sealed.rs` was copied.

## Independent overlay

`cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test a302_sealed -- --test-threads=1`. `linux/proof-1/test.stdout`: `ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.65s`.

Names: `approved_text_reseals_through_the_compiler`, `compiled_cli_matches_the_library`, `hash_tuple_cas_and_stale_field_refuse`, `incomplete_body_does_not_leak`, `occupied_dataset_review_is_unsupported`, `rejected_and_deferred_leave_the_original_seal`, `unclassified_allowlist_is_unknown_pii`.

The overlay content SHA-256 is `bf043d4cecc1a23d1ced7fdb75a4a139bcf69bffa8058858f2d33bffab18d7b1`. It is hand-built for this review. Its canaries are `a302` strings. Sealed constants are source `a302_src`, database `a302db`, captured at `2026-04-11T09:30:00.000000Z` (27 characters), model `a302-sealed`, reviewer `mina.declared`, reviewed on `2026-10-06`, project `a302pack`. The schema preimage is written in the compiler's field order and hashed with `payload_sha256`. The canonical writer uses `format!("\\u{:04x}", u32::from(ch))`. Field hashes go through `payload_sha256` and are checked against `descriptive_field_sha256`. `recompile` calls `compile_captured_pg_catalog` on the canonical catalog, curated overlay, and reviews taken from the evidence envelope.

Results that passed:

- `complete_description_candidates` returns the same bytes on a later call. The review outcome is `reviewed`. Invoice is applied: label disposition `replaced`, description disposition `filled`, grain disposition `kept`. Crate is `reject` and `applied` false. Decisions record `authenticated` false. Review statements include `pii_classification=not_performed` and `database=not_used`.
- The pack outcome is `packed`. `review_sha256` is the SHA-256 of the description-review document. `original_wrapper_hash` equals the compiler wrapper of the submitted artifact. `resealed_wrapper_hash` is a different value. A later `compile_captured_pg_catalog` of the submitted artifact still returns the original wrapper. A `compile_captured_pg_catalog` of `resealed_artifact` returns the wrapper, draft, and evidence-envelope hashes the pack recorded.
- Approved text on the resealed effective invoice is label `Invoice register a302` and description `The badge_code column stores a bearer the gate must not read.` The candidate grain `CANDIDATE_GRAIN_a302_DO_NOT_KEEP` is not that effective grain. The curated invoice document keeps grain `one invoice line a302`, entity `shipment`, `row_count_estimate` 18, tier `raw`, and caveat `kept invoice caveat`. Crate and `public.shelf` curated documents are unchanged.
- PII stays on three layers. The curated invoice column `qty` is `pii` `personal`, `status` `deprecated`, `semantic_type` `quantity`, classification `confirmed` by `ops.lina`. The resealed draft curated overlay has `qty` `pii` `sensitive`, `semantic_type` `free_text`, `status` `draft`. The draft effective `qty` `pii` is null. The reviewed-effective `qty` is `pii` `restricted`, `semantic_type` `quantity`, classification `confirmed` by `mina.owner` at `2026-10-06`. That reviewed value is neither `personal` nor `sensitive`.
- `oo_r_gate` sees invoice `id` and `qty` only. Its role artifact contains the approved label and omits the description, the ident `badge_code`, and the line `> UNREVIEWED: not an approved review, and not a grant.` `oo_r_lane` sees invoice `id`, `qty`, and `badge_code`, plus crate `id`. Its role artifact contains the approved label, the description, `badge_code`, and that UNREVIEWED line. Both artifacts omit the rejected crate label, the rejected crate prose, the candidate grain, and `seal_note`. Pack digests match `compile_role_pack`.
- A wrong `expected_review_sha256` returns `review.hash` and no document. A wrong current `schema_hash` returns `review.schema`. A wrong label `current_sha256` returns `review.cas`. A wrong decision `card_sha256` returns `review.anchor`. An artifact `protocol` of `oo.builder.other` returns `seal.artifact`. Those error strings omit the canaries. The submitted artifact still recompiles to the original wrapper after the artifact refusal.
- Defer invoice and reject crate produce `reviewed_unchanged`, `statements.reseal=not_required`, null reseal fields, and the original curated documents. The pack text omits the approved label and the approved description. The original wrapper is unchanged.
- A captured body `INCOMPLETE_BODY_a302_DO_NOT_LEAK` with status `provider_failure` is absent from the candidate outcome and from the pack text. The pack outcome is `not_candidates`, `compiler_outcome` `provider_failure`, with no `packs`, no `curated_documents`, and no `resealed_artifact`. The queue item is `compiler` / `provider_failure`.
- A sealed review item `dataset/public.invoice` already on the captured artifact makes the pack outcome `unsupported` with seam `review.dataset_decision_occupied`. There is no `packs` key, no `resealed_artifact`, and no `curated_documents`. `statements.reseal` is `unsupported` and `statements.source_artifact` is `not_mutated`. The pack text omits the approved label, the description, and `badge_code`. The original wrapper is unchanged. The overlay does not supply an override that writes through that occupied item.
- Allowlisting the unclassified crate column `seal_note` returns `classification.unknown_pii`. The error omits `seal_note` and the approved description. The original wrapper is unchanged.
- `description-pack apply` stdout equals `apply_description_pack` of the same stdin. Exit 0. Stderr empty. No trailing newline. Usage with no arguments exits 2, writes the usage line on stderr, and leaves stdout empty. A wrong review hash exits 1, leaves stdout empty, and writes `description-pack:` plus `/review.hash` on stderr. The CLI environment contains `ENV_CANARY_a302_DO_NOT_LEAK`. That canary is absent from the refusal stderr.

## Registry triples

`linux/proof-1/lock-subset.stdout` ends with `summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true`. Path packages are `description-pack 0.1.0`, `description-review 0.1.0`, `oo-builder 0.1.0`, `oo-core 0.1.0`, and `oo-jobs 0.1.0`. Direct pins present there include serde `1.0.229`, serde_core `1.0.229`, serde_derive `1.0.229`, serde_json `1.0.151`, and sha2 `0.10.9`. Registry source is `registry+https://github.com/rust-lang/crates.io-index`. The comparator read the child lock and the root lock from the fixed archive. `cargo update` was not invoked. No lock was generated.

Child `Cargo.lock` content SHA-256 `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d`. Root `Cargo.lock` content SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Git blob of that root lock at the fixed commit is `2990f901ea98a7b465f462cce8742bc5c51c13aa`, the same blob as at `216e6df82a61d2bc22548ce1a5075d5f4a045648`. Child lock git blob at the fixed commit is `028fbca1196f70ac69d90616dbc14cbca209b471`. The proof copied the child lock into `linux/proof-1/Cargo.lock` and did not copy it back onto the product tree.

## Archive

Fixed commit `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`, commit tree `5903f9c8f39b9cb426df9c5f88b22af316c7826b`. `tools/description-pack` tree `93e4006b3249fe4849d62f29e53689acb6a2bb6a` is the same at `5ce8b1c4186eb80f9b876a306ae94309f10933ae` and at the fixed commit. `git diff` of `tools/description-pack`, `crates/oo-builder`, `tools/description-review`, and `Cargo.lock` between those two commits is empty. `git log -- tools/description-pack` contains only `5ce8b1c4`. `crates/oo-builder/src/description_candidate.rs` git blob `f783ae95f1f7398da18b6c47bcc87941db49f7a3`. Content SHA-256 of that file in the container is `c392dd9ef7dfff45ea9f6298b257e8fc4bdeca04784acbfbb696ef644f884954`.

`linux/proof-1/archive-blobs.txt` matches the fixed commit for `description_candidate.rs` (`f783ae95f1f7398da18b6c47bcc87941db49f7a3`), `tools/description-pack/src/lib.rs` (`69930324e1a3faa6347eaffc76ed5000966dc348`), and `tools/description-review/src/lib.rs` (`12202a5f4136b37f67786ec1ee42598bd22482b7`).

The archive contains `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `tools/description-review`, and `tools/description-pack`. The toolchain file is moved aside and `RUSTUP_TOOLCHAIN=1.94.0` is set. `.cargo/config.toml` is an xtask alias and is not in the archive. `source/blobs/` is `git archive` of `tools/description-pack` at the fixed commit. Host `shasum -a 256` of those blobs matches `linux/proof-1/pack-source.sha256`. `blob-cmp.exit` is 0. The overlay file is absent from that hash list.

| file | sha256 |
|---|---|
| `CONTRACT.md` | `cf970f86c98282c7b5bdc2a4325339a556965cddb27622e26b0ffcc77fc98592` |
| `Cargo.lock` | `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d` |
| `Cargo.toml` | `85e75020190ce8e56b32756a14c93229e44a34b085db3c960ab6012db5cf212f` |
| `src/apply.rs` | `47c1de40c495e24238562c00d733125cb1e674d5f0818f89670f3d01cef1d225` |
| `src/canonical.rs` | `f43725316fa797c7a3d7eeb0f2c539ddb380a0dd2b7400053a8dd32e94702d6a` |
| `src/error.rs` | `eac4d0679e7920a72f619a093eb3938445485fb8a683921bf4eb1fea8ff1f9d1` |
| `src/lib.rs` | `d7782a147eea84a4b8f6ba38ce10361206758673db4f5fe8979d29fa447cf6a8` |
| `src/main.rs` | `3c85f262a804d13db58f41a01246f95412592830c181d8dfd319d457061cd999` |
| `tests/pack.rs` | `819d5b89bbffe79f79b1285df6f35581459132a7755920e79875547fce1ae02f` |

## Occupied review seam

`review.dataset_decision_occupied` is the outcome the bridge returns when `dataset/public.invoice` is already a sealed review item. The overlay plants that item and expects `unsupported`. It does not forge a merge. That seam stays unresolved for a parent that wants an occupied dataset review to accept a later description decision. Flagging it here is the current explicit behavior. It is not full S5.

## Recipes

Origins and the minimal path changes are in `origin/recipes.txt`. Launch shape is a204 `linux/overlay-3/launch.sh` at `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, blob `03c8e15c60a376ef0a757c062ebd110063a0ffe6`, content SHA-256 `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee`. Review shape is a288 `linux/proof-4/launch.sh`, content SHA-256 `0cf5c779d5b593619aa5b438fc97ef902cc411163d93e5e78e5e30e1734c0157`. The a200 PostgreSQL launch, content SHA-256 `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`, was not executed. `tools/sha256_manifest.rs` is the a199 file, content SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. `tools/lock_subset.rs` is the a292 file, content SHA-256 `dfa03497423b1b35a427aadf6df175fd58d7355ad5c8dab7bb3364c2d31f5454`. It checks the 117 registry triples, the five path packages, and the direct pins above.

This review's `linux/proof-1/launch.sh` content SHA-256 is `0bdb48f5446cd877a3c14f4149d1fb700bb82e14bd917623a6617c4d63f6c89b`. `linux/proof-1/inside.sh` content SHA-256 is `7b50a26ef4fc98859e6698e197342ab2391d3243eb00f0de04697f51a1583e95`.
