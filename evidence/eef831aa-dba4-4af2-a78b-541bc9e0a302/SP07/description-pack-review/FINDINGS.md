# Findings

Reviewer: this session, sole coder and reviewer. This is a separate session from the author of `tools/description-pack`. It is not parent acceptance. The decisive build is `linux/proof-1`. There is no earlier proof directory. The cold-volume offline fetch inside that run exited 101, and the following locked online fetch exited 0. That offline miss is preserved in `linux/proof-1/fetch-offline.stderr`.

## Verdict

The frozen tool matches the checks below on Linux arm64 Rust 1.94.0. The author suite exited 0 before the overlay (8 passed). Pre-overlay Clippy `--locked --offline --all-targets -D warnings` exited 0. The independent overlay exited 0 (7 passed). Post-overlay Clippy with the same flags exited 0. `lock-subset` printed `registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true` against the committed root lock at `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`. Root lock content SHA-256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` was unchanged. Child lock content SHA-256 `2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d` was unchanged. `cargo-update-invoked=no`. No PostgreSQL container was started.

I read the description-pack contract and sources, the description-review apply path, the captured-catalog sealer, the reviewed-effective writer, the role-pack visibility filter, the author REPORT, FINDINGS, and HANDOFF, the a288 review reports, and the proof exits after the container exited.

## What the run shows

The bridge reexecutes `apply_description_review` and binds `expected_review_sha256` to that document. It reseals through `compile_captured_pg_catalog`. Approved label and description are on the new seal. An existing grain with `overwrite` false stays on the curated document, disposition `kept`. The effective grain is not the candidate grain, and the role artifacts do not contain that candidate string. A reject leaves the other curated document unchanged. An extra curated file, `public.shelf`, stays unchanged. Entity, row-count estimate, tier, caveat, column status, and the curated PII value stay on the packed curated documents.

The original captured wrapper hash is the same before the pack call and on a later compiler call of the submitted artifact. The resealed artifact is a different wrapper. Recompiling that resealed artifact through `compile_captured_pg_catalog` reproduces the wrapper, draft, and envelope hashes on the pack document.

PII is three separate values. Curated `qty` is `personal` and `deprecated`, confirmed by `ops.lina`. The draft curated overlay is `sensitive` / `free_text` / `draft`. The draft effective `pii` is null. The reviewed effective value is `restricted`, confirmed by the explicit classification reviewer `mina.owner`. The review document's own statement remains `pii_classification=not_performed`.

A description that names `badge_code` is omitted from the role that cannot see that column, and that role artifact also omits the UNREVIEWED fence. The role that can see `badge_code` keeps the description inside `> UNREVIEWED: not an approved review, and not a grant.`

Wrong expected review hash, wrong current schema hash, wrong label CAS, and wrong decision card hash refuse with `review.hash`, `review.schema`, `review.cas`, and `review.anchor`. A foreign artifact protocol refuses with `seal.artifact`. None of those errors echo the canaries, and none of them replace the original seal.

Incomplete provider text and rejected candidate text stay out of the pack document and out of the role artifacts. Defer plus reject packs the original curated files under `reviewed_unchanged` and sets `reseal` to `not_required`.

An occupied `dataset/public.invoice` review item returns `unsupported` and seam `review.dataset_decision_occupied`. The outcome has no packs and no resealed artifact. The overlay does not invent an override. That seam remains open for the parent. This review does not treat the refusal as full S5.

`description-pack apply` stdout is byte-equal to the library document, with empty stderr and no added newline. Usage exits 2. A bad review hash exits 1 with empty stdout.

## Contract order

`git log -- tools/description-pack` shows one commit, `5ce8b1c4186eb80f9b876a306ae94309f10933ae`. That commit contains `CONTRACT.md` and the sources together. The pack tree id is `93e4006b3249fe4849d62f29e53689acb6a2bb6a` at that commit and at fixed commit `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`. The author FINDINGS state that the contract was written after the first source files existed and before a successful compile, and that the assignment had asked for the contract first. The ledger line in `specs/open-ontology-v0.1-plan.md` and `specs/open-ontology-development-ledger.md` keeps that deviation and says acceptance is pending this review at the fixed commit. S5-D05 remains unchecked. This review records the deviation. It does not rewrite history into a contract-first freeze.

## Limits that stay in force

This run does not open a database, publish a description, authenticate a reviewer, or grant a role. Reviewer label and date are declared inputs. `authenticated` is false on the decision rows. The UNREVIEWED fence is a bound on rendered description text. It is not an approval and it is not a grant.

The occupied-dataset refusal is the bridge's current behavior. A parent that needs a description decision to land on a dataset that already has a sealed review item does not get that behavior from this tool.

S5-D05 in the plan ledger was not changed. A later gate owns publication and full S5.
