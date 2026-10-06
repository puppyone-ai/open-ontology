# S5-D01 description-candidate review

## Verdict

REQUEST_CHANGES.

Fixed commit `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a`, tree `525a432045d17087ee412be0d35550e94c55142a`. Contract `contracts/builder/description-candidate-v1.md`. Producer `crates/oo-builder/src/description_candidate.rs`. This review did not edit product, manifests, locks, or dependencies.

The independent Linux run compiled a fresh sealed captured catalog, prepared a request, and completed a hand-captured response. Prompt, model, budget, and card binding held. Explicit visible tables were requested. Hidden tables, raw samples, and comment bodies stayed out. Truncated, incomplete, provider, and budget responses kept the structural draft and emitted no candidates. Confidence stayed `review_only` and did not write PII, curated, grants, or publication.

`selected_datasets` on the success outcome is not the sorted dataset list the contract requires. See FINDINGS.md.

This is not parent acceptance, not full S5, and not a release. Transport, residency, cost, and column-level candidates stay out of scope.

## How the bytes were built

Smoke and proof built a git archive of `fbc10b04` containing `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `contracts`. The archive mount was read-only. The container copied it to a writable work directory and moved `rust-toolchain.toml` aside so Cargo stayed on image toolchain 1.94.0. `Cargo.lock` remained `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

Workspace HEAD at the proof command was `646e81e39fa2314d20431982021c0d992a7bede1`. Smoke recorded HEAD `827e768b9f94af6134a5734bd16e066ab0d47528`. The archive command was `git archive fbc10b04`, not HEAD.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Each builder had 6 GiB, 1 CPU, `--cap-drop ALL`, `--security-opt no-new-privileges`, `--pull never`, private bridge `oo-a257-eef831aa-net`, `HOME=/tmp`. No PostgreSQL. No model call. No host cargo process.

## Smoke

`linux/smoke` ran the archive with no review overlay. `docker-run.exit` 0. `smoke.exit` 0. Container `oo-a257-smoke` removed. OOMKilled false. Privileged false. Memory 6442450944. NanoCpus 1000000000. NetworkMode `oo-a257-eef831aa-net`.

- lib `description_candidate`: 2 passed, 0 failed
- integration `description_candidate`: 10 passed, 0 failed
- overlay absent
- tree diff is only `rust-toolchain.toml` moved aside
- lock unchanged

Smoke did not run Clippy.

## Proof

`linux/proof` is preserved. The overlay failed to compile (`char` has no `LowerHex` in the preimage escaper), and Clippy was not installed yet. Exit 101. That run is a harness failure. Product sources were not changed.

`linux/proof-2` is the contract run. The corrected overlay was copied only inside the container. Author tests passed. Clippy was installed with `rustup component add clippy --toolchain 1.94.0` on the private network, then `cargo clippy -D warnings` exited 0. Offline fetch exited 0 on the warm cargo volume.

- lib `description_candidate`: 2 passed
- author integration: 10 passed
- independent `description_candidate_a257`: 6 passed, 1 failed
- Clippy exit 0
- lock unchanged
- tree diff is the overlay file plus the toolchain file moved aside
- `docker-run.exit` 101, `proof.exit` 101
- container `oo-a257-proof2` id `e48377ed66dfaed9a9be2c1f25a01161b035cdd00e875f52ced6b654352667b7`, removed
- OOMKilled false, privileged false, memory 6442450944, NanoCpus 1000000000

The failing assertion is in `linux/proof-2/test-a257.log`:

```
success selected_datasets ["public.q_41","public.b"] disagree with the sorted budget outcome ["public.b","public.q_41"]
```

Product file content SHA-256 from that run:

```
68f46e95307efc403164bb71f6cc2a37cd5103c602abd36a9d6b68a9271ea007  description_candidate.rs
c6b5f3204d2b4da278a7194cdda62ba4d2f2310c71bf89450bc71b8d0111fdbf  lib.rs
0882a7c1a2782a16c3e78728c60cb959a4a29b40ca8f16bcbf4bf2a53b07a56e  description_candidate.rs (author test)
bdf41af5a4669103a8d67fde9935921f05bb58651a8c20c5bac0c3cc38cb5908  description-candidate-v1.md
16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a  Cargo.lock
```

The independent test that ran is content SHA-256 `b96a79bb1a27b712458e69cd589f4019af8e1f22c4d2763ca76cf3990dc0e7cc`. The non-compiling proof-1 copy is `301d858ae38270a9c49d44a217bb6c020c3d372a377b620be38601d8dc7cbcc7`, saved at `linux/proof/description_candidate_a257.rs`.

## What held

A fresh catalog (`source_id` `a257_src`, database `a257db`, captured at `2026-10-06T08:30:00.000000Z`) sealed through `compile_captured_pg_catalog`. The caller selected physical tables `b` and `A` and left `vault_secret_table` unselected. Prepare was deterministic. The request protocol, compiler, prompt text, `prompt_sha256`, `budget_sha256`, `request_sha256`, and per-card `card_sha256` matched recomputation. Changing `model_id` changed `request_sha256` and left `card_sha256` unchanged. Cards were ordered by `(schema, name)`. The only neighbor was `b_to_a_fk` to `public.q_41`. The unselected foreign key was omitted. Comment, policy, default, and enum bodies, owner, grantee, and policy name were absent from the request. The comment field was `byte_len` plus `sha256`.

A complete captured response produced two candidates ordered by dataset, `effect=review_only`, confidence `0` and `1.0000`. Labels could contain grant, curated, publication, PII, and approved wording without those becoming fields. Statements stayed on the fixed review-only set.

Truncated, incomplete, provider_failure, and caller `budget_failure` dropped the body and kept the draft hashes with `candidates=[]`. Duplicate keys, unknown keys (`publication`, `curated`, `grants`, `pii`), a candidate `pii` field, a flipped `request_sha256`, a wrong `card_sha256`, a missing dataset, an extra vault dataset, numeric confidence, and confidence `1.0001` emitted no candidates. Column, request-byte, and response-byte budgets kept the draft. Unknown, duplicate, and hidden selections returned `Err` and did not echo the rejected names. A 31-byte schema name compiled with an empty generated-dataset list and was refused as `selection.hidden`.

No model network call and no PostgreSQL session were used.
