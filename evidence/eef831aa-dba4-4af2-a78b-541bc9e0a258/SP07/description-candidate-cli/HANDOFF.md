# Handoff

## This unit

Fixed archive `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a`.

Contract commit `2abc6c1a14401ce1b2165090dbaef9f4aba8bec5`.
Implementation commit `827e768b9f94af6134a5734bd16e066ab0d47528`.

Contract: `contracts/builder/description-candidate-cli-v1.md`.

Binary: `description-candidate`.

Phases:

- `prepare_request` calls `prepare_description_request`
- `complete_candidates` calls `complete_description_candidates`

Passing proof: `linux/proof`. Pre-edit smoke: `linux/preoverlay`. Both are Linux arm64 Rust 1.94, 6 GiB, 1 CPU, no PostgreSQL. Lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

Inventory: `SHA256SUMS`, rewritten by `linux/manifest/finalize.sh` after the compile log and exit files were closed. The tool is `tools/sha256_manifest.rs`, content-identical to the a199 blob `f21989651570187fc286462349d59174a6a6b8ef`.

## Next child

Model transport, residency approval, and cost are not implemented. A later child has to supply `captured_response` from an approved provider. This CLI will pass that document to `complete_description_candidates` only when the operator names `complete_candidates`.

Do not treat `confidence` as approval or as a PII classification. `effect` remains `review_only`. A transport failure must use status `truncated`, `incomplete`, `provider_failure`, or `budget_failure` so the structural draft hashes are preserved and no candidate is emitted.

## Do not widen this unit

Column-level candidates, raw samples, comment bodies, extra flags, and edits to `Cargo.lock`, the library, or the connector manifest are outside the owned paths. A new expansion needs its own frozen contract before code.

This child does not claim parent acceptance, full S5, publication, or residency.
