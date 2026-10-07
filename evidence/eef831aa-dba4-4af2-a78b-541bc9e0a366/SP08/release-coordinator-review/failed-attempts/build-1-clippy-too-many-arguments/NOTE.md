# Build 1

The immutable archive baseline finished before the review oracle was judged. Author Clippy exit 0, author tests exit 0, author binary build exit 0. Independent tests `review_01` through `review_13` passed (review-test.exit 0). Review Clippy exit 101. Docker exit 1. Host config recorded oom=false.

The only Clippy diagnostic was `too_many_arguments` on `record_gate` (8/7) in the review copy of `src/review_oracle.rs`. That helper is review evidence, not product code. The rerun wraps the same fields in `GateIn` through `gate_in` (6 parameters). Schemas `oo_release_control` on `oo_a366` and `oo_a366_author` were dropped before the rerun so stage sees a fresh database. Roles and databases stayed.
