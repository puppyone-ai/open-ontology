# description-review independent review

Verdict: **APPROVE_LIMITED** for `tools/description-review` at implementation `8f4e792c4eca6eabb204de4c8bc09caa854165ec`, archived from fixed commit `216e6df82a61d2bc22548ce1a5075d5f4a045648`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a288`. This directory is the independent review record. The author session `eef831aa-dba4-4af2-a78b-541bc9e0a277` and its `linux/proof-4` are the author's run. They are not this review. No product file, root lock, spec, or ledger was edited. This file does not accept S5-D04 for the parent, does not accept publication, does not accept reviewer authentication, does not close SP07, and does not certify a release.

The catalog ident of table `A` is `q_41`. The sealed datasets are `public.b`, `public.q_41`, and an extra curated dataset `public.aside`.

## Decisive run

`linux/proof-4` is one container, `oo-a288-proof4`, id `ac9c102020e928e021eec8e1bed072dcb8fb30dec2c7b2f6761ba1ec3bfdd006`. HostConfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a288-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`. `capacity.txt` recorded `own=0 global=1 host_cargo=0 cap_global=10 cap_own=1 postgres=0` before start. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0` `(85eff7c80 2026-01-15)`. Debian 13. Kernel line `Linux ac9c102020e9 6.12.76-linuxkit #1 SMP Fri Apr 17 14:56:37 UTC 2026 aarch64 GNU/Linux`. `repo-head.txt` is `2a6b6f0acd51c060577e1f336e0a686d54df2001`. The archive is the fixed commit, not that HEAD.

`docker-run.exit` is 0. `version.exit` 0, `rustc-release.txt` `1.94.0`, `blob-cmp.exit` 0, `overlay-absent.exit` 0, `lock.exit` `present`, `lock-compile.exit` 0, `lock-subset.exit` 0, `lock-summary.exit` 0. `fetch-offline.exit` 0 and `fetch.exit` 0 on the private cargo volume filled by `linux/proof-2`. `smoke-test.exit` 0 before the overlay was copied, `smoke-count.txt` 16, `smoke-names.exit` 0. `overlay-present.exit` 0, `test.exit` 0. `clippy-component.exit` 0, `clippy.exit` 0, `clippy.stdout` empty, Clippy stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 1.38s`. `product-unchanged.exit` 0. `cargo-update-invoked.txt` is `no`. `python-present.flag` is 1. The scripts did not invoke Python. PostgreSQL was not started.

## Preserved attempts

`linux/proof` is the first attempt, container `oo-a288-proof1`, id `551d023b7fe8a290d866e1dd77d46c1870533daae4a4485f3f307f1ffd1bf887`. ExitCode 2. `version.exit` 2. `rustc -vV` printed `release: 1.94.0`, then `grep -q` under `set -o pipefail` closed the pipe. No tests ran. `linux/proof/failure-note.txt` records that. The crate was not edited.

`linux/proof-2`, container `oo-a288-proof2`, id `93f89ad00fe5951bb033e8da6c50488fff65e0a348cfc471cd5fcdf5f5627446`. ExitCode 101. Author smoke exited 0 with 16 passed. Lock subset exited 0. Offline fetch exited 101 (`no matching package named serde_json` on the cold volume). Online `cargo fetch --locked` exited 0. The overlay did not compile: `E0106` on `dataset_item` and `decision_row`. Clippy exited 101 on that compile error. `product-unchanged.exit` 0.

`linux/proof-3`, container `oo-a288-proof3`, id `bce2af9f733ab3daf5427830d798543279ded8f92ba56ffdaa59311bd5aae4ff`. ExitCode 101. Author smoke exited 0 with 16 passed. Offline fetch exited 0. Overlay `cargo test --test a288_sealed` exited 0, 6 passed. Clippy exited 101 on `dead_code` for `Produced.outcome_text` and `OneField.lower`. `product-unchanged.exit` 0.

## Author smoke, before the overlay

`cargo test --manifest-path tools/description-review/Cargo.toml --locked --offline -- --test-threads=1` on the sealed archive, with `tests/a288_sealed.rs` absent. `linux/proof-4/smoke-test.stdout` sums to 16 passed. Library and doctest rows are 0. The 16 names:

`field_literals_are_independent_of_the_library_helper`, `approve_one_dataset_and_reject_the_other`, `defer_and_a_missing_decision_leave_curated_text`, `cas_mismatch_does_not_write_and_a_quoted_literal_does`, `stale_schema_and_evidence_block_every_dataset`, `one_stale_anchor_does_not_block_the_other_dataset`, `a_missing_curated_file_is_not_invented`, `decision_schema_and_candidate_hash_block_one_dataset`, `provider_failure_does_not_copy_the_captured_body`, `wrong_outcome_hash_and_a_compiler_error_return_no_document`, `unknown_keys_do_not_echo_an_injected_token`, `exact_core_bounds_apply_and_a_longer_label_is_not_truncated`, `a_backtick_description_is_refused_without_a_partial_write`, `reviewer_length_is_a_command_error_and_256_is_kept`, `duplicate_unknown_and_identity_fail_closed`, `cli_apply_matches_the_library_and_usage_stays_on_stderr`.

`linux/proof-2` and `linux/proof-3` recorded the same 16 before their overlay Clippy failures. `linux/proof` did not reach those tests.

## Independent overlay

`cargo test --manifest-path tools/description-review/Cargo.toml --locked --offline --test a288_sealed -- --test-threads=1`. `linux/proof-4/test.stdout`: `ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s`.

Names: `sealed_reexecute_overwrites_one_field_and_reject_keeps_confidence`, `defer_and_missing_decision_preserve_current_curated`, `stale_schema_evidence_card_request_curated_and_candidate_do_not_mutate`, `unknown_classification_and_status_keys_are_refused`, `bounds_apply_whole_and_are_not_truncated`, `compiled_cli_equals_the_library_document`.

`apply_ok` checks every returned document. The command string is unchanged. The document has no added newline. Canonical JSON of the parsed value equals the returned text. `review_sha256` is SHA-256 of that canonical JSON with `review_sha256` removed. `authenticated` is false on every decision row. `confidence_effect` is `recorded_only`.

Sealed constants are model `a288-sealed`, owner `seal_owner`, captured at `2026-03-15T08:30:00.000000Z` (27 characters), source `a288_src`, database `a288db`, reviewer `nia.declared`, reviewed on `2026-03-15`. `ident("A")` is `q_41`. `ident("b")` is `b`.

Results that passed:

- `complete_description_candidates` on the immutable command, including `captured_response`, returns the same bytes on a second call. Those bytes equal the overlay canonical JSON. `outcome_sha256` on the review is `payload_sha256` of those bytes. Outcome name is `candidates`. Selected datasets are `public.b` and `public.q_41`. Each candidate `effect` is `review_only`.
- Approve `public.b` with description `overwrite` true and the current field hash replaces only `description` with `sealed replacement for one field`. Label `overwrite` false keeps `Sealed current label`. Omitted grain stays `sealed current grain` and the disposition is `not_selected`. `changed` is true. `document_sha256` matches the canonical document. `validate_dataset` is empty and a serde round trip equals the document.
- Reject `public.q_41` with confidence `"1"` leaves that document unchanged. `applied` is false. Dispositions are `unchanged`. `public.aside` is unchanged. Candidate text `must stay off` and `Upper candidate` are absent from the written documents. The queue is `public.b` / `pending` / `caveats`, then `public.b` / `pending` / `open_questions`.
- Columns stay on the document: first column `pii` `none` and `semantic_type` `identifier`; second column `pii` `personal`, `semantic_type` `phone`, classification `confirmed` by `ops.mina`, `status` `deprecated`, enum `{schema}.{table}.code`, description `sealed phone note`. Curated caveat `sealed curated caveat`, entity `customer`, tier `raw`, and `row_count_estimate` 17 stay. The document has no `open_questions`, `confidence`, `classification`, `pii`, or `status` keys.
- Defer `public.q_41` with confidence `"1"` and no decision for `public.b` leave both documents and `public.aside` unchanged. The only decision row is the deferral. Its `candidate_label` is `Deferred candidate text`. That text is absent from the `DatasetFile`. `Missing decision candidate text` is absent from the review JSON. The queue is `decision_missing` for `public.b`, then `deferred` for `public.q_41`.
- A current schema hash of `a` repeated to 64 characters, with both decisions approve, leaves both documents unchanged and queues `stale` / `schema` for each. A current evidence hash of `b` repeated queues `stale` / `evidence` for each and leaves both documents unchanged.
- A stale request, card, curated, or candidate anchor on `public.b` leaves that document unchanged, queues only that reason for `public.b`, and still replaces the `public.q_41` label with `Fresh upper label`. The upper description, grain, columns, and caveats stay the curated values. `Fresh lower` is absent from the lower document.
- Unknown `classification` and `status` keys on the review command, each set to `A288_CLASS_STATUS_TOKEN`, return `schema.invalid`. The error text does not contain the token. The same keys on a candidate make `complete_description_candidates` return `format_rejected` with compiler statement `curated=not_written`. The review outcome is `not_candidates`, the curated document is unchanged, the token is absent from the review JSON, and the queue is `compiler` / `format_rejected`. The captured command string still contains the token.
- Label, description, and grain of 256 `N`, 4000 `M`, and 256 `R` are written whole. Lengths are those counts. `validate_dataset` is empty. The rejected upper document is unchanged.
- A 257-character label prefixed `A288LONG` is `format_rejected`. The review outcome is `not_candidates`. The review JSON does not contain `A288LONG` or 256 `N`. The curated label stays `Short lower`.
- Description `A288_CTRL` plus U+0001 is `format_rejected` with compiler `refusal` `type`. The review JSON does not contain `A288_CTRL`. The curated description stays `short lower desc`. Outcome is `not_candidates`.
- `description-review apply` stdout equals `apply_description_review` of the same stdin. Exit 0. Stderr empty. No trailing newline. The same one-field documents, compiler statements, review statements, and rejected confidence `"1"` are on that stdout.

Compiler statements on these documents are the candidate compiler's statements: `curated=not_written`, `auto_accept=not_authorized`, `confidence_effect=recorded_only`, `publication=unavailable`, `llm_transport=not_used`. That object has no `database` key and no `authentication` key. The review document's own `statements` object is `authentication=not_claimed`, `auto_accept=not_authorized`, `column_acl=not_changed`, `confidence_effect=recorded_only`, `database=not_used`, `effective=not_written`, `entity=not_changed`, `files=not_written`, `grants=not_authorized`, `metric=not_changed`, `owner_ui=not_claimed`, `pii_classification=not_performed`, `publication=unavailable`, `reviewer=declared_only`, `semantic_classification=not_performed`, `source_artifact=not_mutated`, `status=not_changed`. That object has no `curated` key and no `llm_transport` key.

## Registry triples

`linux/proof-4/lock-subset.stdout` ends with `summary registry_triples=117 missing=0 mismatch=0 path_packages=4 direct_pins_present=true`. Path packages are `description-review 0.1.0`, `oo-builder 0.1.0`, `oo-core 0.1.0`, and `oo-jobs 0.1.0`. Direct pins present there include serde `1.0.229`, serde_core `1.0.229`, serde_derive `1.0.229`, serde_json `1.0.151`, and sha2 `0.10.9`. Registry source is `registry+https://github.com/rust-lang/crates.io-index`. The comparator read the child lock and the root lock from the fixed archive. `cargo update` was not invoked. No lock was generated.

Child `Cargo.lock` content SHA-256 `eed8dd34b188b9fa6a22d907373cab8242b48014d6dc21878c24828c01f011dc`. Root `Cargo.lock` content SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Git blob of that root lock at the fixed commit is `2990f901ea98a7b465f462cce8742bc5c51c13aa`. The proof copied the child lock into `linux/proof-4/Cargo.lock` and did not copy it back onto the product tree. `src-before.txt` matches `src-after.txt`.

## Archive

Fixed commit `216e6df82a61d2bc22548ce1a5075d5f4a045648`, commit tree `b7d5faef17d3e8f0170047cc7f82a996f46538ff`. `tools/description-review` tree `16a7de40a57a7db335b5a0a005b8239360aa678d` is the same at `8f4e792c4eca6eabb204de4c8bc09caa854165ec` and at the fixed commit. `git diff` of that path between those commits is empty. `crates/oo-builder/src/description_candidate.rs` git blob `f783ae95f1f7398da18b6c47bcc87941db49f7a3` at both. Content SHA-256 of that file in the container is `c392dd9ef7dfff45ea9f6298b257e8fc4bdeca04784acbfbb696ef644f884954`.

`linux/proof-4` archives `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, and `tools/description-review`. The toolchain file is moved aside and `RUSTUP_TOOLCHAIN=1.94.0` is set. `.cargo/config.toml` is an xtask alias and is not in the archive. `source/blobs/` is `git archive` of `tools/description-review` at the fixed commit. `blob-cmp.exit` is 0.

| file | sha256 |
|---|---|
| `.gitignore` | `44c92e3a70ad3307b7056871c2bdb096d8bfa9373f5bf06a79bb6324a20ff2fb` |
| `CONTRACT.md` | `8c12d2a46e0c37c197d1d2c4959774b1f5fb57f20ae815a06d2e6261de6d1a12` |
| `Cargo.lock` | `eed8dd34b188b9fa6a22d907373cab8242b48014d6dc21878c24828c01f011dc` |
| `Cargo.toml` | `75408b1c0d01a7b0c923d560b1de5b1594b86692da0eb32d6d541e73093fb4d1` |
| `src/apply.rs` | `24137358a89c6db87ffbcdcc02abd4f86a218945170136fa5e91a0a6bad05255` |
| `src/canonical.rs` | `0552989bac3a25d0a1c0cda64b0f8125f9d26068690b609bf8ed45846030086f` |
| `src/error.rs` | `6e60ce376d9ce83d0a792ee26b2afe3aa2b9df91626c7f4835e41f58315161c8` |
| `src/lib.rs` | `c1b0a352e31adb96f316fe956fedd765ac98419b0043fc36d4ceb6de88e042f3` |
| `src/main.rs` | `e481be10ed8b070bef56f963855072bfbb32852ff12d68efdfad8573e568ffb6` |
| `tests/review.rs` | `db02fb04e873f7f5d027f07af6b6ae26a5cba25bcd257bf725be688cc0c79f28` |

## Recipes

Origins and the minimal path changes are in `origin/recipes.txt`. Launch shape is a204 `linux/overlay-3/launch.sh` at `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, blob `03c8e15c60a376ef0a757c062ebd110063a0ffe6`, content SHA-256 `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee`. The pre-overlay order follows a204 `linux/preoverlay/launch.sh` through a283. The a200 PostgreSQL launch, content SHA-256 `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`, was not executed. `tools/sha256_manifest.rs` is the a199 file, content SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. `tools/lock_subset.rs` checks the 117 registry triples, the four path packages, and the direct pins above.
