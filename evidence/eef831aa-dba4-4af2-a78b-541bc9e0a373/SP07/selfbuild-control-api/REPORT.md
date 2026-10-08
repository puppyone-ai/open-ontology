# SP07 B04 self-build control API

Session `eef831aa-dba4-4af2-a78b-541bc9e0a373`. Linux arm64, Rust `1.94.0 (4a4ef493e 2026-03-02)`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Private Postgres `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` container `oo-a373-pg` stayed up. Databases were not dropped.

The final builder exited 0 in 113.60s. `cargo clippy --locked --offline --all-targets -- -D warnings` exited 0. `OO_SELFBUILD_PHASE=failure` exited 0 in 0.71s. `OO_SELFBUILD_PHASE=success` exited 0. `workflow-100k.exit` is 0. Raw files are under `linux/raw/workflow-100k/`.

## Shop fixture

Failure job `b_c8b4a42a5484fd235a0a9a7e9e6bd88f` (`shopfailkey0000002`) stays failed at S5 `generate.failed`. The replay ran with the model forwarder off. Pack on that job is HTTP 503 `dependency.not_ready`. Anonymous GET is 401 `unauthenticated`. Reviewer create, cross-project, and auditor write are 403. A wrong revision is 409 `revision.conflict`. Create replay is 202 for the same build. A different allowance on the same key is 403 `forbidden` before idempotency.

Success job `b_f36a05605491fe53429435921539158c` (`shopsuccesskey0001`) was not requeued. S0–S8 succeeded. S9 is `awaiting`. S10 is `succeeded` because the owner confirm marked that checkpoint. The job row remains `running` at stage column S5. Generation outcome is `validated`, cache `partial`, model hits 2, misses 1. Cache replay hits 4, misses 0, exit 0. Prompt tokens 15044, completion tokens 20538. Declared cost is `unknown` on every call. Suggestions stay unmerged: `auto_merge` false, `published` false, `metrics_certified` false, and the document has no `"certified": true`.

Reviewer `fixture-reviewer` has projects beta and shop, capabilities `identity.current` and `review.decide`. Approve of `shop.party.display_name` is 200, then replay 200. The same key with choice defer is 409 `idempotency.conflict`. Defer of `shop.entry.memo` is 200. Reviewer confirm and reviewer pack are 403. Hidden `shop.party.phone` is 409 `membership.refused`. Unknown PII is 409 `pii.unknown`. Accept-all preview is 409 `batch.accept_all`. Preview `wrote` is false. Owner confirm of the 12 visible columns is 200, `grants` `not_authorized`, `ledger_authenticated` false. Ledger `oo_review_shop` holds those 12 confirms, one approve, and one defer. Every ledger `authenticated` value is false.

`compileBuildPack` is 200. Roles are `oo_r_analyst` and `oo_r_clerk`. `compiled_pack` is `not_published`. `published` is false. `producer_authenticated` is false. `field_candidates_applied` is false. `reseal_count` is 0 and the producer outcome is `reviewed_unchanged`. After the server process was killed and started again, GET pack is 200 with the same `output_sha256`. Pack file SHA-256 is `5cba6c681aa4e8b5d6ece88dade8e7577286a7cd3010008fd9cd726688186b08`.

Cancel build `b_ad9189fb55129fb4447c4743584be58f` (`shopcancelkey000001`) was created only after the success pack. The test observed status `running`, POST cancel returned 202, and the job reached `canceled` at S0. GET pack on that build is 403.

The product model origin stays `http://127.0.0.1:8317`. The success phase used a test-only TCP forwarder inside the builder from that loopback address to `host.docker.internal:8317`. The failure phase ran before the forwarder started.

## Still open

Original E56, SP07, the parent goal, customer IdP, publication, the harness, and the full role matrix remain open. Identity is the unaccepted test issuer from archive `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. Contract `089b00d7f70da1bc42f7e201dfa070748c651d5b` was not changed. S9 has no complete label review. The producer did not reseal the draft. The role text has no `UNREVIEWED` banner because the sealed draft and curated files have no description or caveat prose; declared foreign keys say they are source-declared and not a grant. `tests/contract.rs` was clippy-checked and its assertions were not executed in this run.
