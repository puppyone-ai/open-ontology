# Findings

Reviewer: this session, sole coder and reviewer. This is a separate session from the author of `tools/description-review`. It is not parent acceptance. The decisive build is `linux/proof-4`. `linux/proof`, `linux/proof-2`, and `linux/proof-3` are preserved.

## Verdict

The frozen tool matches the checks below on Linux arm64 Rust 1.94.0. The author suite exited 0 before the overlay (16 passed). The independent overlay exited 0 (6 passed). Clippy `--locked --offline --all-targets -D warnings` exited 0. `lock-subset` printed `registry_triples=117 missing=0 mismatch=0 path_packages=4 direct_pins_present=true` against the committed root lock at `216e6df82a61d2bc22548ce1a5075d5f4a045648`. Root lock content SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` was unchanged. Child lock content SHA-256 `eed8dd34b188b9fa6a22d907373cab8242b48014d6dc21878c24828c01f011dc` was unchanged. `cargo-update-invoked=no`. No PostgreSQL container was started.

I read the description-review contract and sources, the candidate compiler outcome shape, `DatasetFile` validation, the author REPORT, FINDINGS, and HANDOFF, and the proof exits after those containers exited.

## What the runs show

The review reexecutes `oo_builder::complete_description_candidates` on the immutable command string. A second call returns the same bytes. Those bytes are canonical JSON. The review `outcome_sha256` is the SHA-256 of that payload. The compiler's copied statements include `curated=not_written`, `auto_accept=not_authorized`, `confidence_effect=recorded_only`, `publication=unavailable`, and `llm_transport=not_used`. The review document's own statements are a different object: `database=not_used`, `authentication=not_claimed`, `files=not_written`, `publication=unavailable`, `reviewer=declared_only`. Decisions record `authenticated` false. Confidence `"1"` on a reject and on a defer does not write.

Approve with a matching current hash replaces one selected field. In the sealed command that field is `description`. Label with `overwrite` false stays. Omitted grain stays, disposition `not_selected`. Columns, the curated caveat, entity, tier, and row-count estimate stay on the document. Generated caveats and open questions are on the decision and the queue. They are not written onto the `DatasetFile`. An extra curated dataset, `public.aside`, stays unchanged. A missing decision and a defer leave the current curated documents unchanged. The missing-decision candidate text is absent from the review JSON.

A mismatched current schema hash, or a mismatched current evidence hash, leaves every dataset unchanged. A mismatched request, card, curated, or candidate anchor on one dataset leaves that dataset unchanged and still allows the other dataset's approved label write. Unknown `classification` and `status` keys on the review command are `schema.invalid` and the injected token is not in the error. The same keys on a candidate are compiler `format_rejected`. The review outcome is `not_candidates`, the curated file is unchanged, and the token is absent from the review document.

Bounds of 256, 4000, and 256 scalars are applied whole and `validate_dataset` is empty. A 257-character label is refused whole. The review JSON does not contain a truncated prefix of that label. A description containing U+0001 is compiler `format_rejected` with `refusal` `type`. The review does not copy that text, and the curated description stays whole. `description-review apply` stdout is byte-equal to the library document, with empty stderr and no added newline.

## Preserved failures

`linux/proof` stopped in the 1.94.0 check. `grep -q` under `pipefail` treated a closed `rustc -vV` pipe as failure after the release line had already been printed. The crate was not edited.

`linux/proof-2` ran the author 16 tests, fetched with `--locked`, and then failed to compile the overlay. `dataset_item` and `decision_row` returned a reference and took two references, so a fully elided signature is `E0106`. Clippy stopped on that compile error. The crate was not edited.

`linux/proof-3` compiled and passed the six overlay tests. Clippy denied `dead_code` on two overlay fields that were stored and never read. Those fields are absent from the overlay that `linux/proof-4` ran. The crate was not edited.

## Limits that stay in force

This run does not open a database, publish a description, or authenticate a reviewer. Reviewer label and date are declared inputs. `confidence` does not approve and does not classify PII. Candidate `effect` stays `review_only`. Compiler `curated=not_written` describes the historical candidate outcome. It is not a claim that this review wrote a database.

The U+0001 case is the compiler's `refusal` `type` on a `format_rejected` outcome, then a review result of `not_candidates`. This overlay does not drive the review's own core-type branch with a candidates outcome that already passed the compiler. The author suite's backtick refusal ran before the overlay. This review did not open a provider or a database attack set.

`validate_dataset` on the applied exact-bound document was empty. That check, in the core this archive builds, is the money-unit check. It is not a second copy of the text bound.

S5-D04 in the plan ledger was not changed. A later gate owns publication, a live provider, and full S5.
