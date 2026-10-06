# Review 01

Scope: `tools/pg-onboarding` at `1b314d73d5a105898921e453ef0382fdad96d069`, the frozen contract, and the a261 evidence for this CLI. Boundary checks are the preserved smoke, post-edit, and live exits. No product crate was reviewed as a change, because this unit did not edit one.

Verdict: APPROVE_LIMITED

## What holds

Inspect and apply are separate phases. Inspect writes `outcome.json` and the sealed manifest. Apply writes `apply-outcome.json` and does not replace the inspect outcome. Pre-checks that reject a bad manifest return before creating `apply-outcome.json`. After apply steps start, a failure leaves the step files and the draft.

`bind_generation` returns the source cut and errors when either side is below 1 or the two sides are equal. The live manifest shows cut 48 and fence 1. The generation was taken from import stdout.

The config digest in that manifest equals the digest recomputed from `source_id`, host, port, database, username, `secret_ref`, the default import budget, and the two selected tables. Password and CA bytes are not inputs.

Apply calls `review-pg-draft`, `reviewed-effective-oos`, `reviewed-policy`, `install-column-acl`, `provision-mapping-reader`, `analyze-pg`, `role-pack`, `role-pack-bundle`, and `query-pg`. Allow lists and visibility files come from the operator config. The structural review file is written by the proof from the captured queue, not by the library.

The live oracle passed on a fresh TLS cluster: unknown rows refused before analyze, hidden and write queries refused after the bundles, and the positive reads returned the int8 amount extremes. A second apply was rejected with `terminal`.

Focused tests and Clippy on Rust 1.94 were clean in `linux/postedit-attempt-4` before that live run. The live edit after that run is the statement timeout constant, committed as `1b314d73` and then executed.

## Limits of this approval

The approval is the onboarding CLI's two phases on one two-table fixture. It is not acceptance of SP11-W06 as an ops console, and it is not semantic publication, authentication, natural language, or a harness mount.

`database_state` on a failed apply still says `unchanged_by_this_tool` after compilers have written to the target. Rollback and release flags stay false. Readers must use the step directory to see how far the children got. Chain attempt 1 is the example: analyze failed after earlier children had run, and that database was discarded with the private volume rather than rolled back by the CLI.

`analyze-pg` accepts a statement timeout only through 30000 ms. The first live config requested 60000 and the child returned exit 3 `bound`. The proof now requests 30000. The CLI still forwards the configured number and relies on that child to reject a larger one.

Duplicate JSON keys follow serde_json last-wins. The subprocess capture redacts a secret after the child exits; it does not wipe process memory.

The runtime role membership grant is outside the CLI. Shipping this binary does not grant business columns by itself. An operator still has to supply the review files and a trustworthy config.
