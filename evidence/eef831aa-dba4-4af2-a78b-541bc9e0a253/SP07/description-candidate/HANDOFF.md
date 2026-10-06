# Handoff

## This unit

Source commit `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a` on fixed `3f8807b495bd78ee1d8c99aca41c54c3137778c6`.

Contract: `contracts/builder/description-candidate-v1.md`.

API:

- `prepare_description_request`
- `complete_description_candidates`
- `DESCRIPTION_REQUEST_COMMAND_PROTOCOL`
- `DESCRIPTION_REQUEST_PROTOCOL`
- `DESCRIPTION_RESPONSE_PROTOCOL`
- `DESCRIPTION_CANDIDATE_PROTOCOL`
- `DESCRIPTION_CANDIDATE_COMPILER`
- `DESCRIPTION_PROMPT_VERSION`

Passing proof: `linux/proof-7`. Pre-edit smoke: `linux/preoverlay`. Both are Linux arm64 Rust 1.94, 6 GiB, 1 CPU, no PostgreSQL. Lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

Inventory: `SHA256SUMS`, rewritten by `linux/manifest/finalize.sh` after the compile log and exit files were closed. The tool is `tools/sha256_manifest.rs`.

## Next child

Model transport, residency approval, and cost are not implemented. A later child has to supply the captured response from an approved provider. This compiler will accept that response only when the caller passes it in `captured_response` and the body echoes the sealed request, prompt, model, source, budget, and card hashes.

Do not treat `confidence` as approval or as a PII classification. Statements stay compiler-owned. `effect` is `review_only`. A transport failure must use status `truncated`, `incomplete`, `provider_failure`, or `budget_failure` so the structural draft hashes are preserved and no candidate is emitted.

## Do not widen this unit

Column-level candidates, raw samples, comment bodies, CLI flags, and edits to `Cargo.lock` or the connector manifest are outside the owned paths. A new expansion needs its own frozen contract before code.

This child does not claim parent acceptance, full S5, publication, or residency.
