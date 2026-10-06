# Findings

Reviewer: this session, sole coder and reviewer. This is not an independent review and it is not parent acceptance.

## Verdict

The frozen child matches `linux/proof-4`. That run is Linux arm64 Rust 1.94, implementation commit `8f4e792c4eca6eabb204de4c8bc09caa854165ec`, whose ancestor is the contract commit `7f500ff0a61a42ea606a281dd931099091a52ef6`. Focused tests exited 0 (16 integration). Clippy `--all-targets -D warnings` exited 0. The child lock's registry triples are a subset of the root lock (`missing=0 mismatch=0`). The root lock hash did not change.

I read the proof-4 phase exits, `test.log`, `clippy.log`, `lock-subset.txt`, `lock-hash.txt`, and `hostconfig.txt` after the container exited. I did not start a second warm product container.

## What the proof shows

The tests compile a sealed physical `public.A` / `public.b` catalog, prepare a description request, and complete it with `complete_description_candidates`. The review command embeds that full command string. The library runs the compiler again and checks the outcome hash. Approving `public.b` replaces the label, keeps the description, and fills grain. Rejecting `public.q_41` leaves it unchanged, including a confidence value of `"1"`. Columns, PII, semantic type, classification, status, enum, entity, tier, and the existing caveat list stay on the typed document. Generated caveats and open questions are queue items. An extra curated dataset is preserved. Defer, a missing decision, a CAS miss, a stale schema or evidence hash, a one-sided request, card, curated, decision-schema, or candidate-hash miss, a missing curated file, a provider-failure body, a wrong outcome hash, a compiler error, an unknown key, a 257-character reviewer label, and a backtick description behave as the contract states. Exact 256 / 4000 / 256 text is applied whole. The CLI usage and apply paths match the library document.

## Limits that stay in force

The review labels are declared strings. `authenticated` on each decision is false. There is no owner UI and no authentication proof. Confidence is stored and does not select a write. Publication, residency, grants, and a customer-owner proof are outside this child. The preoverlay `description_selection_order` pass is one test on the fixed archive. It does not accept full S5.

`linux/proof`, `linux/proof-2`, and `linux/proof-3` are retained compile or assertion failures. proof-3 had already passed 15 tests before the empty-queue assertion failed. Those directories are preserved.

## Not accepted here

Parent acceptance, release, full S5, a live provider, and a certificate of residency or ownership are outside this child.
